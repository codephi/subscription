use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::usage::CreateUsageEventRequest,
    error::{ApiError, ApiResult},
    repositories::usage_models::{
        Conversion, CycleWindow, LockedMeter, PendingQuote, PriceDefinition, PriceTier, PricedBlock,
    },
    services::calendar::pricing_cycle_bounds,
};

pub(super) async fn calculate_usage(
    transaction: &mut Transaction<'_, Postgres>,
    request: &CreateUsageEventRequest,
    meter: &LockedMeter,
    accepted_at: DateTime<Utc>,
) -> ApiResult<Conversion> {
    let active = load_active_price(transaction, request.item_id, accepted_at).await?;
    ensure_expected_price(request.expected_price_version_id, active.price_id)?;
    let mut calculation = Calculation::new(request.item_units.value());
    complete_pending(transaction, meter, accepted_at, &mut calculation).await?;
    consume_active(transaction, meter, &active, accepted_at, &mut calculation).await?;
    let conversion = calculation.finish()?;
    validate_meter_growth(meter, request.item_units.value(), &conversion)?;
    Ok(conversion)
}

struct Calculation {
    remaining_received: i64,
    blocks: Vec<PricedBlock>,
    pending_after: i64,
    pending_quote: Option<PendingQuote>,
    converted_units: i64,
    debited_credits: i64,
}

impl Calculation {
    fn new(received: i64) -> Self {
        Self {
            remaining_received: received,
            blocks: Vec::new(),
            pending_after: 0,
            pending_quote: None,
            converted_units: 0,
            debited_credits: 0,
        }
    }

    fn push_block(&mut self, block: PricedBlock) -> ApiResult<()> {
        self.converted_units = checked_add(
            "converted item units",
            self.converted_units,
            block.block_size,
        )?;
        self.debited_credits = checked_add(
            "debited credit units",
            self.debited_credits,
            block.credit_units,
        )?;
        self.blocks.push(block);
        Ok(())
    }

    fn set_pending(&mut self, units: i64, quote: PendingQuote) {
        self.pending_after = units;
        self.pending_quote = Some(quote);
        self.remaining_received = 0;
    }

    fn finish(self) -> ApiResult<Conversion> {
        if self.remaining_received != 0 {
            return Err(ApiError::unexpected(format!(
                "usage calculation left {} item units unassigned",
                self.remaining_received
            )));
        }
        Ok(Conversion {
            blocks: self.blocks,
            converted_units: self.converted_units,
            pending_after: self.pending_after,
            pending_quote: self.pending_quote,
            debited_credits: self.debited_credits,
        })
    }
}

async fn complete_pending(
    transaction: &mut Transaction<'_, Postgres>,
    meter: &LockedMeter,
    accepted_at: DateTime<Utc>,
    calculation: &mut Calculation,
) -> ApiResult<()> {
    if meter.pending == 0 {
        return Ok(());
    }
    let quote = pending_quote(meter)?;
    let missing = quote.block_size.checked_sub(meter.pending).ok_or_else(|| {
        ApiError::unexpected(format!(
            "pending {} must be below block size {}",
            meter.pending, quote.block_size
        ))
    })?;
    if calculation.remaining_received < missing {
        let pending = checked_add(
            "pending item units",
            meter.pending,
            calculation.remaining_received,
        )?;
        calculation.set_pending(pending, quote);
        return Ok(());
    }
    calculation.remaining_received -= missing;
    let price = load_price(transaction, quote.price_id).await?;
    let block = price_block(transaction, meter, &price, &quote, accepted_at).await?;
    calculation.push_block(block)
}

async fn consume_active(
    transaction: &mut Transaction<'_, Postgres>,
    meter: &LockedMeter,
    price: &PriceDefinition,
    accepted_at: DateTime<Utc>,
    calculation: &mut Calculation,
) -> ApiResult<()> {
    while calculation.remaining_received > 0 {
        let quote = active_quote(transaction, meter, price, accepted_at).await?;
        if calculation.remaining_received < quote.block_size {
            calculation.set_pending(calculation.remaining_received, quote);
            break;
        }
        calculation.remaining_received -= quote.block_size;
        let block = price_block(transaction, meter, price, &quote, accepted_at).await?;
        calculation.push_block(block)?;
    }
    Ok(())
}

async fn active_quote(
    transaction: &mut Transaction<'_, Postgres>,
    meter: &LockedMeter,
    price: &PriceDefinition,
    accepted_at: DateTime<Utc>,
) -> ApiResult<PendingQuote> {
    if price.model == "unit" {
        return Ok(PendingQuote {
            price_id: price.price_id,
            tier_position: None,
            block_size: required_price_value(price.block_size, price.price_id, "unit_block_size")?,
            credit_units: required_price_value(price.credit_units, price.price_id, "credit_units")?,
        });
    }
    let cycle = cycle_window(price, accepted_at)?;
    let (accumulated, _) = lock_accumulator(transaction, meter, price, &cycle).await?;
    let tier = find_tier(price, accumulated)?;
    Ok(quote_from_tier(price.price_id, tier))
}

async fn price_block(
    transaction: &mut Transaction<'_, Postgres>,
    meter: &LockedMeter,
    price: &PriceDefinition,
    quote: &PendingQuote,
    accepted_at: DateTime<Utc>,
) -> ApiResult<PricedBlock> {
    let cycle = cycle_window(price, accepted_at)?;
    let (accumulated, converted_blocks) =
        lock_accumulator(transaction, meter, price, &cycle).await?;
    let accumulated_after = checked_add("pricing accumulator", accumulated, quote.block_size)?;
    let price_block_ordinal = converted_blocks.checked_add(1).ok_or_else(|| {
        ApiError::unprocessable(
            "usage_arithmetic_overflow",
            format!("price block ordinal {converted_blocks} cannot be incremented"),
        )
    })?;
    advance_accumulator(transaction, meter, price, &cycle, quote.block_size).await?;
    Ok(PricedBlock {
        price_id: price.price_id,
        model: price.model.clone(),
        tier_position: quote.tier_position,
        block_size: quote.block_size,
        credit_units: quote.credit_units,
        cycle,
        accumulated_before: accumulated,
        accumulated_after,
        price_block_ordinal,
    })
}

async fn load_active_price(
    transaction: &mut Transaction<'_, Postgres>,
    item_id: Uuid,
    accepted_at: DateTime<Utc>,
) -> ApiResult<PriceDefinition> {
    let price_id = sqlx::query_scalar(
        "SELECT price_version_id FROM price_versions WHERE item_id=$1 \
         AND state IN ('ACTIVE','SCHEDULED') AND effective_from<=$2 \
         AND (effective_until IS NULL OR effective_until>$2) \
         ORDER BY effective_from DESC LIMIT 1",
    )
    .bind(item_id)
    .bind(accepted_at)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        ApiError::conflict(
            "active_price_not_found",
            format!("item {item_id} has no active price at {accepted_at}"),
        )
    })?;
    load_price(transaction, price_id).await
}

async fn load_price(
    transaction: &mut Transaction<'_, Postgres>,
    price_id: Uuid,
) -> ApiResult<PriceDefinition> {
    let row = sqlx::query(
        "SELECT pricing_model,unit_block_size,credit_units,accumulation_anchor_at, \
         accumulation_recurrence_rule FROM price_versions WHERE price_version_id=$1 FOR SHARE",
    )
    .bind(price_id)
    .fetch_one(&mut **transaction)
    .await?;
    let tier_rows = sqlx::query(
        "SELECT position,from_accumulated_units,to_accumulated_units,unit_block_size,credit_units \
         FROM price_tiers WHERE price_version_id=$1 ORDER BY position",
    )
    .bind(price_id)
    .fetch_all(&mut **transaction)
    .await?;
    Ok(PriceDefinition {
        price_id,
        model: row.get("pricing_model"),
        block_size: row.get("unit_block_size"),
        credit_units: row.get("credit_units"),
        anchor_at: row.get("accumulation_anchor_at"),
        recurrence_rule: row.get("accumulation_recurrence_rule"),
        tiers: tier_rows.iter().map(tier_from_row).collect(),
    })
}

fn tier_from_row(row: &sqlx::postgres::PgRow) -> PriceTier {
    PriceTier {
        position: row.get("position"),
        from: row.get("from_accumulated_units"),
        to: row.get("to_accumulated_units"),
        block_size: row.get("unit_block_size"),
        credit_units: row.get("credit_units"),
    }
}

fn cycle_window(price: &PriceDefinition, at: DateTime<Utc>) -> ApiResult<CycleWindow> {
    let Some(anchor) = price.anchor_at else {
        return Ok(CycleWindow {
            key: "lifetime".to_string(),
            start: None,
            end: None,
            anchor_at: None,
            recurrence_rule: None,
        });
    };
    let rule = price.recurrence_rule.as_deref().ok_or_else(|| {
        ApiError::unexpected(format!("price {} cycle rule is absent", price.price_id))
    })?;
    let (start, end) = pricing_cycle_bounds(anchor, rule, at)?;
    Ok(CycleWindow {
        key: start.to_rfc3339(),
        start: Some(start),
        end: Some(end),
        anchor_at: Some(anchor),
        recurrence_rule: Some(rule.to_string()),
    })
}

async fn lock_accumulator(
    transaction: &mut Transaction<'_, Postgres>,
    meter: &LockedMeter,
    price: &PriceDefinition,
    cycle: &CycleWindow,
) -> ApiResult<(i64, i64)> {
    sqlx::query(
        "INSERT INTO pricing_accumulators (pricing_accumulator_id,customer_id,item_id, \
         price_version_id,cycle_key) SELECT $1,w.customer_id,pv.item_id,pv.price_version_id,$3 \
         FROM price_versions pv JOIN wallets w ON w.item_id=pv.item_id \
         WHERE pv.price_version_id=$2 AND w.wallet_type='ITEM' AND w.wallet_id=$4 \
         ON CONFLICT (customer_id,item_id,price_version_id,cycle_key) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(price.price_id)
    .bind(&cycle.key)
    .bind(meter.wallet_id)
    .execute(&mut **transaction)
    .await?;
    let row = sqlx::query(
        "SELECT accumulated_converted_item_units,converted_blocks FROM pricing_accumulators \
         WHERE price_version_id=$1 AND cycle_key=$2 AND customer_id=(SELECT customer_id FROM wallets WHERE wallet_id=$3) \
         FOR UPDATE",
    )
    .bind(price.price_id)
    .bind(&cycle.key)
    .bind(meter.wallet_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok((
        row.get("accumulated_converted_item_units"),
        row.get("converted_blocks"),
    ))
}

async fn advance_accumulator(
    transaction: &mut Transaction<'_, Postgres>,
    meter: &LockedMeter,
    price: &PriceDefinition,
    cycle: &CycleWindow,
    block_size: i64,
) -> ApiResult<()> {
    let updated = sqlx::query(
        "UPDATE pricing_accumulators SET accumulated_converted_item_units= \
         accumulated_converted_item_units+$3,converted_blocks=converted_blocks+1,version=version+1 \
         ,updated_at=clock_timestamp() \
         WHERE price_version_id=$1 AND cycle_key=$2 AND customer_id= \
         (SELECT customer_id FROM wallets WHERE wallet_id=$4)",
    )
    .bind(price.price_id)
    .bind(&cycle.key)
    .bind(block_size)
    .bind(meter.wallet_id)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() == 1 {
        return Ok(());
    }
    Err(ApiError::unexpected(format!(
        "price {} accumulator {} was not updated",
        price.price_id, cycle.key
    )))
}

fn pending_quote(meter: &LockedMeter) -> ApiResult<PendingQuote> {
    Ok(PendingQuote {
        price_id: required_price_value(
            meter.pending_price_id,
            meter.wallet_id,
            "pending_price_version_id",
        )?,
        tier_position: meter.pending_tier_position,
        block_size: required_price_value(
            meter.pending_block_size,
            meter.wallet_id,
            "pending_unit_block_size",
        )?,
        credit_units: required_price_value(
            meter.pending_credit_units,
            meter.wallet_id,
            "pending_credit_units",
        )?,
    })
}

fn find_tier(price: &PriceDefinition, accumulated: i64) -> ApiResult<&PriceTier> {
    price
        .tiers
        .iter()
        .find(|tier| tier.from <= accumulated && tier.to.is_none_or(|end| accumulated < end))
        .ok_or_else(|| {
            ApiError::unexpected(format!(
                "price {} has no tier for accumulated units {accumulated}",
                price.price_id
            ))
        })
}

fn quote_from_tier(price_id: Uuid, tier: &PriceTier) -> PendingQuote {
    PendingQuote {
        price_id,
        tier_position: Some(tier.position),
        block_size: tier.block_size,
        credit_units: tier.credit_units,
    }
}

fn ensure_expected_price(expected: Option<Uuid>, active: Uuid) -> ApiResult<()> {
    if expected.is_none_or(|value| value == active) {
        return Ok(());
    }
    Err(ApiError::conflict(
        "price_version_changed",
        format!("expected price {expected:?}, found active price {active}"),
    ))
}

fn required_price_value<T>(value: Option<T>, id: Uuid, field: &str) -> ApiResult<T> {
    value.ok_or_else(|| ApiError::unexpected(format!("resource {id} requires {field}")))
}

fn checked_add(label: &str, left: i64, right: i64) -> ApiResult<i64> {
    left.checked_add(right).ok_or_else(|| {
        ApiError::unprocessable(
            "usage_arithmetic_overflow",
            format!("{label} {left} plus {right} exceeds signed 64-bit range"),
        )
    })
}

fn validate_meter_growth(
    meter: &LockedMeter,
    received: i64,
    conversion: &Conversion,
) -> ApiResult<()> {
    checked_add("total received item units", meter.total_received, received)?;
    checked_add(
        "total converted item units",
        meter.total_converted,
        conversion.converted_units,
    )?;
    let new_blocks = i64::try_from(conversion.blocks.len()).map_err(|_| {
        ApiError::unprocessable(
            "usage_arithmetic_overflow",
            format!(
                "converted block count {} exceeds signed 64-bit range",
                conversion.blocks.len()
            ),
        )
    })?;
    checked_add("total converted blocks", meter.total_blocks, new_blocks)?;
    Ok(())
}
