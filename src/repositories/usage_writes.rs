use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::usage::CreateUsageEventRequest,
    error::{ApiError, ApiResult},
    repositories::{
        usage_models::{Conversion, DebitResult, LockedMeter, PricedBlock},
        usage_outbox::insert_usage_outbox,
        usage_references::insert_usage_references,
    },
};

pub(super) async fn apply_usage_debit(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    usage_event_id: Uuid,
    meter: &LockedMeter,
    conversion: &Conversion,
    request: &CreateUsageEventRequest,
    accepted_at: DateTime<Utc>,
) -> ApiResult<DebitResult> {
    let balance_before: i64 = sqlx::query_scalar(
        "SELECT balance_credit_units FROM customer_wallets WHERE wallet_id=$1 FOR UPDATE",
    )
    .bind(meter.customer_wallet_id)
    .fetch_one(&mut **transaction)
    .await?;
    let balance_after = balance_before
        .checked_sub(conversion.debited_credits)
        .ok_or_else(|| ApiError::unexpected("wallet debit overflow"))?;
    if balance_after < 0 {
        return Err(ApiError::conflict(
            "insufficient_credit",
            format!(
                "workspace {workspace_id} balance {balance_before} cannot cover {} credits",
                conversion.debited_credits
            ),
        ));
    }
    let debit = insert_debit_entry(
        transaction,
        workspace_id,
        usage_event_id,
        meter,
        conversion,
        request,
        balance_before,
        balance_after,
    )
    .await?;
    allocate_credit_lots(
        transaction,
        workspace_id,
        debit.debit_id,
        conversion.debited_credits,
        accepted_at,
    )
    .await?;
    Ok(debit)
}

#[allow(clippy::too_many_arguments)]
async fn insert_debit_entry(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    usage_event_id: Uuid,
    meter: &LockedMeter,
    conversion: &Conversion,
    request: &CreateUsageEventRequest,
    balance_before: i64,
    balance_after: i64,
) -> ApiResult<DebitResult> {
    let entry_id = Uuid::new_v4();
    let debit_id = Uuid::new_v4();
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(entry_sequence),0)+1 FROM customer_wallet_entries \
         WHERE customer_wallet_id=$1",
    )
    .bind(meter.customer_wallet_id)
    .fetch_one(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO customer_wallet_entries (customer_wallet_entry_id,customer_wallet_id, \
         customer_id,entry_sequence,entry_type,source_channel,signed_credit_units, \
         balance_before_credit_units,balance_after_credit_units,transaction_id,metadata,request_id) \
         VALUES ($1,$2,$3,$4,'DEBIT','usage',$5,$6,$7,$8,$9,$10)",
    )
    .bind(entry_id)
    .bind(meter.customer_wallet_id)
    .bind(workspace_id)
    .bind(sequence)
    .bind(-conversion.debited_credits)
    .bind(balance_before)
    .bind(balance_after)
    .bind(&request.transaction_id)
    .bind(request.metadata.clone().unwrap_or_else(|| json!({})))
    .bind(Uuid::new_v4())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE customer_wallets SET balance_credit_units=$2,version=version+1 WHERE wallet_id=$1",
    )
    .bind(meter.customer_wallet_id)
    .bind(balance_after)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO debits (debit_id,usage_event_id,customer_wallet_entry_id,debited_credit_units) \
         VALUES ($1,$2,$3,$4)",
    )
    .bind(debit_id)
    .bind(usage_event_id)
    .bind(entry_id)
    .bind(conversion.debited_credits)
    .execute(&mut **transaction)
    .await?;
    Ok(DebitResult {
        debit_id,
        entry_id,
        entry_sequence: sequence,
        balance_before,
        balance_after,
    })
}

async fn allocate_credit_lots(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    debit_id: Uuid,
    required: i64,
    accepted_at: DateTime<Utc>,
) -> ApiResult<()> {
    let rows = sqlx::query(
        "SELECT credit_lot_id,remaining_credit_units FROM credit_lots WHERE customer_id=$1 \
         AND remaining_credit_units>0 AND (expires_at IS NULL OR expires_at>$2) \
         ORDER BY CASE WHEN source_kind='SUBSCRIPTION' AND expires_at IS NOT NULL THEN 0 \
           WHEN expires_at IS NULL THEN 1 ELSE 2 END,expires_at NULLS LAST,created_at,credit_lot_id FOR UPDATE",
    )
    .bind(workspace_id)
    .bind(accepted_at)
    .fetch_all(&mut **transaction)
    .await?;
    let mut remaining = required;
    for (index, row) in rows.iter().enumerate() {
        if remaining == 0 {
            break;
        }
        let available: i64 = row.get("remaining_credit_units");
        let allocated = available.min(remaining);
        persist_lot_allocation(
            transaction,
            row.get("credit_lot_id"),
            debit_id,
            allocated,
            index,
        )
        .await?;
        remaining -= allocated;
    }
    if remaining == 0 {
        return Ok(());
    }
    Err(ApiError::conflict(
        "insufficient_credit",
        format!("workspace {workspace_id} eligible credit lots are short by {remaining} for debit {required}"),
    ))
}

async fn persist_lot_allocation(
    transaction: &mut Transaction<'_, Postgres>,
    lot_id: Uuid,
    debit_id: Uuid,
    allocated: i64,
    index: usize,
) -> ApiResult<()> {
    let ordinal = i64::try_from(index)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| ApiError::unexpected(format!("allocation index {index} exceeds bigint")))?;
    sqlx::query(
        "UPDATE credit_lots SET remaining_credit_units=remaining_credit_units-$2 \
         WHERE credit_lot_id=$1",
    )
    .bind(lot_id)
    .bind(allocated)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO credit_lot_allocations (credit_lot_allocation_id,debit_id,credit_lot_id, \
         allocated_credit_units,allocation_ordinal) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(debit_id)
    .bind(lot_id)
    .bind(allocated)
    .bind(ordinal)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn persist_usage(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    item_entry_id: Uuid,
    workspace_id: Uuid,
    request: &CreateUsageEventRequest,
    accepted_at: DateTime<Utc>,
    meter: &LockedMeter,
    conversion: &Conversion,
    debit: Option<&DebitResult>,
) -> ApiResult<()> {
    let received_after = meter
        .total_received
        .checked_add(request.item_units.value())
        .ok_or_else(|| {
            ApiError::unprocessable("item_units_overflow", "total received item units overflow")
        })?;
    insert_usage_event(
        transaction,
        usage_id,
        workspace_id,
        request,
        accepted_at,
        meter,
        conversion,
        received_after,
    )
    .await?;
    insert_item_entry(
        transaction,
        usage_id,
        item_entry_id,
        request,
        accepted_at,
        meter,
        conversion,
        debit,
        received_after,
    )
    .await?;
    update_item_meter(transaction, usage_id, meter, conversion, received_after).await?;
    insert_billing_blocks(
        transaction,
        usage_id,
        item_entry_id,
        workspace_id,
        request.item_id,
        meter,
        conversion,
        debit,
    )
    .await?;
    if let Some(debit) = debit {
        insert_usage_references(transaction, debit.entry_id, usage_id, request, meter, debit)
            .await?;
    }
    insert_usage_outbox(
        transaction,
        workspace_id,
        usage_id,
        meter,
        conversion,
        debit,
        accepted_at,
        request.item_units.value(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_usage_event(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    workspace_id: Uuid,
    request: &CreateUsageEventRequest,
    accepted_at: DateTime<Utc>,
    meter: &LockedMeter,
    conversion: &Conversion,
    received_after: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO usage_events (usage_event_id,customer_id,item_wallet_id,transaction_id, \
         product_id,item_id,item_units,expected_price_version_id,occurred_at,accepted_at,metadata, \
         pending_item_units_before,converted_item_units,converted_blocks,pending_item_units_after, \
         unit_offset_start,unit_offset_end,debited_credit_units) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
    )
    .bind(usage_id)
    .bind(workspace_id)
    .bind(meter.wallet_id)
    .bind(&request.transaction_id)
    .bind(request.product_id)
    .bind(request.item_id)
    .bind(request.item_units.value())
    .bind(request.expected_price_version_id)
    .bind(request.occurred_at)
    .bind(accepted_at)
    .bind(request.metadata.clone().unwrap_or_else(|| json!({})))
    .bind(meter.pending)
    .bind(conversion.converted_units)
    .bind(i64::try_from(conversion.blocks.len()).unwrap_or(i64::MAX))
    .bind(conversion.pending_after)
    .bind(meter.total_received)
    .bind(received_after)
    .bind(conversion.debited_credits)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_item_entry(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    entry_id: Uuid,
    request: &CreateUsageEventRequest,
    accepted_at: DateTime<Utc>,
    meter: &LockedMeter,
    conversion: &Conversion,
    debit: Option<&DebitResult>,
    received_after: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO item_wallet_entries (item_wallet_entry_id,item_wallet_id,usage_event_id, \
         transaction_id,received_item_units,total_received_item_units_before,total_received_item_units_after, \
         converted_item_units,converted_blocks,pending_item_units_after,emitted_debited_credit_units, \
         unit_offset_start,unit_offset_end,debit_id,customer_wallet_entry_id,accepted_at,metadata) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
    )
    .bind(entry_id)
    .bind(meter.wallet_id)
    .bind(usage_id)
    .bind(&request.transaction_id)
    .bind(request.item_units.value())
    .bind(meter.total_received)
    .bind(received_after)
    .bind(conversion.converted_units)
    .bind(i64::try_from(conversion.blocks.len()).unwrap_or(i64::MAX))
    .bind(conversion.pending_after)
    .bind(conversion.debited_credits)
    .bind(meter.total_received)
    .bind(received_after)
    .bind(debit.map(|value| value.debit_id))
    .bind(debit.map(|value| value.entry_id))
    .bind(accepted_at)
    .bind(request.metadata.clone().unwrap_or_else(|| json!({})))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn update_item_meter(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    meter: &LockedMeter,
    conversion: &Conversion,
    received_after: i64,
) -> Result<(), sqlx::Error> {
    let pending = conversion.pending_quote.as_ref();
    sqlx::query(
        "UPDATE item_wallets SET total_received_item_units=$2, \
         total_converted_item_units=total_converted_item_units+$3, \
         total_converted_blocks=total_converted_blocks+$4,pending_item_units=$5, \
         pending_price_version_id=$6,pending_tier_position=$7,pending_unit_block_size=$8, \
         pending_credit_units=$9,last_usage_event_id=$10,version=version+1,updated_at=clock_timestamp() \
         WHERE wallet_id=$1",
    )
    .bind(meter.wallet_id)
    .bind(received_after)
    .bind(conversion.converted_units)
    .bind(i64::try_from(conversion.blocks.len()).unwrap_or(i64::MAX))
    .bind(conversion.pending_after)
    .bind(pending.map(|quote| quote.price_id))
    .bind(pending.and_then(|quote| quote.tier_position))
    .bind(pending.map(|quote| quote.block_size))
    .bind(pending.map(|quote| quote.credit_units))
    .bind(usage_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_billing_blocks(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    item_entry_id: Uuid,
    workspace_id: Uuid,
    item_id: Uuid,
    meter: &LockedMeter,
    conversion: &Conversion,
    debit: Option<&DebitResult>,
) -> ApiResult<()> {
    let Some(debit) = debit else {
        return Ok(());
    };
    let mut offset = meter.total_converted;
    for (index, block) in conversion.blocks.iter().enumerate() {
        insert_billing_block(
            transaction,
            usage_id,
            item_entry_id,
            workspace_id,
            item_id,
            meter,
            debit,
            block,
            i64::try_from(index).unwrap_or(i64::MAX),
            offset,
        )
        .await?;
        offset = offset
            .checked_add(block.block_size)
            .ok_or_else(|| ApiError::unexpected("billing block offset overflow"))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_billing_block(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    item_entry_id: Uuid,
    workspace_id: Uuid,
    item_id: Uuid,
    meter: &LockedMeter,
    debit: &DebitResult,
    block: &PricedBlock,
    index: i64,
    offset: i64,
) -> ApiResult<()> {
    let global_sequence = meter
        .total_blocks
        .checked_add(index)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| ApiError::unexpected("global billing block sequence overflow"))?;
    let offset_end = offset
        .checked_add(block.block_size)
        .ok_or_else(|| ApiError::unexpected("billing block offset overflow"))?;
    sqlx::query(
        "INSERT INTO billing_blocks (billing_block_id,customer_id,item_wallet_id,item_id, \
         global_block_sequence,price_version_id,price_block_ordinal,cycle_key,tier_position, \
         cycle_start,cycle_end,accumulation_anchor_at,accumulation_recurrence_rule, \
         accumulated_units_before,accumulated_units_after,unit_block_size,debited_credit_units, \
         unit_offset_start,unit_offset_end,usage_event_id,item_wallet_entry_id,debit_id,customer_wallet_entry_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(meter.wallet_id)
    .bind(item_id)
    .bind(global_sequence)
    .bind(block.price_id)
    .bind(block.price_block_ordinal)
    .bind(&block.cycle.key)
    .bind(block.tier_position)
    .bind(block.cycle.start)
    .bind(block.cycle.end)
    .bind(block.cycle.anchor_at)
    .bind(&block.cycle.recurrence_rule)
    .bind(block.accumulated_before)
    .bind(block.accumulated_after)
    .bind(block.block_size)
    .bind(block.credit_units)
    .bind(offset)
    .bind(offset_end)
    .bind(usage_id)
    .bind(item_entry_id)
    .bind(debit.debit_id)
    .bind(debit.entry_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
