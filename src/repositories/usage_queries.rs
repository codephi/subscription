use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{
        units::{CreditUnits, ItemUnitBoundary, ItemUnits},
        usage::{
            BillingBlockResponse, ItemWalletEntryResponse, ItemWalletMeterResponse,
            ItemWalletStatementResponse, PricingAccumulatorResponse, ProductEligibilityResponse,
            UsageReconciliationResponse,
        },
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
    services::calendar::pricing_cycle_bounds,
};

impl DatabaseRepository {
    pub async fn find_product_eligibility(
        &self,
        workspace_id: Uuid,
        product_id: Uuid,
    ) -> ApiResult<ProductEligibilityResponse> {
        let mut transaction = self.pool().begin().await?;
        super::credits::lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        let product = sqlx::query("SELECT usage_model,status FROM products WHERE product_id=$1")
            .bind(product_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| missing("product", product_id))?;
        let evaluated_at: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await?;
        let plan = sqlx::query(
            "SELECT commercial_status,activation_status FROM customer_plans WHERE customer_id=$1 \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(workspace_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let status = plan
            .as_ref()
            .map(|row| row.get::<String, _>("commercial_status"));
        let usable = plan.as_ref().is_some_and(|row| {
            matches!(
                row.get::<String, _>("commercial_status").as_str(),
                "ACTIVE" | "ACTIVE_PAID" | "PAST_DUE"
            ) && row.get::<String, _>("activation_status") == "ACTIVATED"
        });
        let entitled =
            Self::product_entitled(&mut transaction, workspace_id, product_id, evaluated_at)
                .await?;
        let wallet = sqlx::query(
            "SELECT cw.balance_credit_units,cw.version FROM customer_wallets cw JOIN wallets w \
             ON w.wallet_id=cw.wallet_id WHERE w.customer_id=$1",
        )
        .bind(workspace_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let balance = wallet
            .as_ref()
            .map(|row| row.get::<i64, _>("balance_credit_units"));
        let product_active = product.get::<String, _>("status") == "ACTIVE";
        let (eligible, reason) = eligibility_reason(product_active, usable, entitled, balance);
        transaction.commit().await?;
        Ok(ProductEligibilityResponse {
            product_id,
            usage_model: product.get("usage_model"),
            eligible,
            reason: reason.to_string(),
            renewal_status: status.as_deref().map(renewal_status).map(str::to_string),
            commercial_status: status,
            entitled,
            balance_credit_units: balance.map(CreditUnits::new),
            wallet_version: wallet.as_ref().map(|row| row.get("version")),
            evaluated_at,
        })
    }

    async fn product_entitled(
        transaction: &mut Transaction<'_, Postgres>,
        workspace_id: Uuid,
        product_id: Uuid,
        evaluated_at: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM customer_plan_entitlements e JOIN customer_plans c \
             ON c.customer_plan_id=e.customer_plan_id WHERE c.customer_id=$1 AND e.product_id=$2 \
             AND e.effective_from<=$3 AND (e.effective_until IS NULL OR e.effective_until>$3) \
             AND c.commercial_status IN ('ACTIVE','ACTIVE_PAID','PAST_DUE') \
             AND c.activation_status='ACTIVATED')",
        )
        .bind(workspace_id)
        .bind(product_id)
        .bind(evaluated_at)
        .fetch_one(&mut **transaction)
        .await
    }

    pub async fn find_item_wallet_meter(
        &self,
        workspace_id: Uuid,
        item_id: Uuid,
    ) -> ApiResult<ItemWalletMeterResponse> {
        let row = sqlx::query(
            "SELECT w.wallet_id,w.parent_customer_wallet_id,i.product_id,iw.total_received_item_units, \
             iw.total_converted_item_units,iw.total_converted_blocks,iw.pending_item_units, \
             iw.pending_price_version_id,iw.pending_tier_position,iw.pending_unit_block_size, \
             iw.pending_credit_units,iw.version,iw.updated_at FROM wallets w JOIN item_wallets iw ON iw.wallet_id=w.wallet_id \
             JOIN items i ON i.item_id=w.item_id WHERE w.customer_id=$1 AND w.item_id=$2",
        )
        .bind(workspace_id)
        .bind(item_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing_item_wallet(workspace_id, item_id))?;
        let pending: i64 = row.get("pending_item_units");
        let quote = if pending > 0 {
            (
                row.get("pending_price_version_id"),
                row.get("pending_tier_position"),
                row.get("pending_unit_block_size"),
                row.get("pending_credit_units"),
            )
        } else {
            self.next_active_quote(workspace_id, item_id).await?
        };
        let block_size: i64 = required_quote(quote.2, item_id, "unit_block_size")?;
        let next_price_id = required_quote(quote.0, item_id, "price_version_id")?;
        let pricing_accumulators = self
            .list_pricing_accumulators(workspace_id, item_id, None)
            .await?;
        Ok(ItemWalletMeterResponse {
            customer_id: workspace_id,
            product_id: row.get("product_id"),
            item_id,
            item_wallet_id: row.get("wallet_id"),
            parent_customer_wallet_id: row.get("parent_customer_wallet_id"),
            total_received_item_units: boundary(row.get("total_received_item_units")),
            total_converted_item_units: boundary(row.get("total_converted_item_units")),
            total_converted_blocks: row.get("total_converted_blocks"),
            pending_item_units: boundary(pending),
            pending_price_version_id: row.get("pending_price_version_id"),
            pending_tier_position: row.get("pending_tier_position"),
            pending_unit_block_size: row
                .get::<Option<i64>, _>("pending_unit_block_size")
                .map(ItemUnits::positive)
                .transpose()?,
            next_price_version_id: next_price_id,
            units_until_next_block: ItemUnits::positive(block_size - pending)?,
            next_block_credit_units: CreditUnits::new(required_quote(
                quote.3,
                item_id,
                "credit_units",
            )?),
            pricing_accumulators,
            version: row.get("version"),
            updated_at: row.get("updated_at"),
        })
    }

    async fn next_active_quote(
        &self,
        workspace_id: Uuid,
        item_id: Uuid,
    ) -> ApiResult<(Option<Uuid>, Option<i32>, Option<i64>, Option<i64>)> {
        let price = sqlx::query(
            "SELECT price_version_id,pricing_model,unit_block_size,credit_units, \
             accumulation_anchor_at,accumulation_recurrence_rule FROM price_versions \
             WHERE item_id=$1 AND state IN ('ACTIVE','SCHEDULED') AND effective_from<=now() \
             AND (effective_until IS NULL OR effective_until>now()) ORDER BY effective_from DESC LIMIT 1",
        )
        .bind(item_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| ApiError::conflict("active_price_not_found", format!("item {item_id} has no active price")))?;
        let price_id: Uuid = price.get("price_version_id");
        if price.get::<String, _>("pricing_model") == "unit" {
            return Ok((
                Some(price_id),
                None,
                price.get("unit_block_size"),
                price.get("credit_units"),
            ));
        }
        let cycle_key = price_cycle_key(&price)?;
        let accumulated: i64 = sqlx::query_scalar(
            "SELECT COALESCE((SELECT accumulated_converted_item_units FROM pricing_accumulators \
             WHERE customer_id=$1 AND item_id=$2 AND price_version_id=$3 AND cycle_key=$4),0)",
        )
        .bind(workspace_id)
        .bind(item_id)
        .bind(price_id)
        .bind(cycle_key)
        .fetch_one(&self.pool())
        .await?;
        let tier = sqlx::query(
            "SELECT position,unit_block_size,credit_units FROM price_tiers WHERE price_version_id=$1 \
             AND from_accumulated_units<=$2 AND (to_accumulated_units IS NULL OR to_accumulated_units>$2) \
             ORDER BY position LIMIT 1",
        )
        .bind(price_id)
        .bind(accumulated)
        .fetch_one(&self.pool())
        .await?;
        Ok((
            Some(price_id),
            Some(tier.get("position")),
            Some(tier.get("unit_block_size")),
            Some(tier.get("credit_units")),
        ))
    }

    pub async fn list_item_wallet_statement(
        &self,
        workspace_id: Uuid,
        item_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> ApiResult<ItemWalletStatementResponse> {
        let rows = sqlx::query(
            "SELECT e.*,u.product_id,u.item_id FROM item_wallet_entries e JOIN usage_events u \
             ON u.usage_event_id=e.usage_event_id JOIN wallets w ON w.wallet_id=e.item_wallet_id \
             WHERE w.customer_id=$1 AND w.item_id=$2 AND ($3::bigint IS NULL OR \
             e.total_received_item_units_after<$3) ORDER BY e.total_received_item_units_after DESC LIMIT $4",
        )
        .bind(workspace_id)
        .bind(item_id)
        .bind(cursor)
        .bind(limit + 1)
        .fetch_all(&self.pool())
        .await?;
        let has_more = rows.len() as i64 > limit;
        let mut items = Vec::with_capacity(rows.len().min(limit as usize));
        for row in rows.iter().take(limit as usize) {
            items.push(self.item_entry_from_row(row).await?);
        }
        let next_cursor = has_more
            .then(|| {
                items
                    .last()
                    .map(|entry| entry.total_received_item_units_after.value().to_string())
            })
            .flatten();
        Ok(ItemWalletStatementResponse { items, next_cursor })
    }

    pub async fn find_item_wallet_entry(
        &self,
        workspace_id: Uuid,
        item_id: Uuid,
        entry_id: Uuid,
    ) -> ApiResult<ItemWalletEntryResponse> {
        let row = sqlx::query(
            "SELECT e.*,u.product_id,u.item_id FROM item_wallet_entries e JOIN usage_events u \
             ON u.usage_event_id=e.usage_event_id JOIN wallets w ON w.wallet_id=e.item_wallet_id \
             WHERE w.customer_id=$1 AND w.item_id=$2 AND e.item_wallet_entry_id=$3",
        )
        .bind(workspace_id)
        .bind(item_id)
        .bind(entry_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing("item_wallet_entry", entry_id))?;
        self.item_entry_from_row(&row).await
    }

    async fn item_entry_from_row(
        &self,
        row: &sqlx::postgres::PgRow,
    ) -> ApiResult<ItemWalletEntryResponse> {
        let entry_id: Uuid = row.get("item_wallet_entry_id");
        let block_rows = sqlx::query(
            "SELECT * FROM billing_blocks WHERE item_wallet_entry_id=$1 ORDER BY global_block_sequence",
        )
        .bind(entry_id)
        .fetch_all(&self.pool())
        .await?;
        let billing_blocks: Vec<BillingBlockResponse> = block_rows
            .iter()
            .map(billing_block_from_row)
            .collect::<ApiResult<_>>()?;
        Ok(ItemWalletEntryResponse {
            item_wallet_entry_id: entry_id,
            usage_event_id: row.get("usage_event_id"),
            product_id: row.get("product_id"),
            item_id: row.get("item_id"),
            transaction_id: row.get("transaction_id"),
            received_item_units: ItemUnits::positive(row.get("received_item_units"))?,
            total_received_item_units_before: boundary(row.get("total_received_item_units_before")),
            total_received_item_units_after: boundary(row.get("total_received_item_units_after")),
            converted_item_units: boundary(row.get("converted_item_units")),
            converted_blocks: row.get("converted_blocks"),
            pending_item_units_after: boundary(row.get("pending_item_units_after")),
            emitted_debited_credit_units: CreditUnits::new(row.get("emitted_debited_credit_units")),
            debit_id: row.get("debit_id"),
            customer_wallet_entry_id: row.get("customer_wallet_entry_id"),
            billing_block_ids: billing_blocks
                .iter()
                .map(|block| block.billing_block_id)
                .collect(),
            billing_blocks,
            metadata: row.get("metadata"),
            accepted_at: row.get("accepted_at"),
        })
    }

    pub async fn list_pricing_accumulators(
        &self,
        workspace_id: Uuid,
        item_id: Uuid,
        price_id: Option<Uuid>,
    ) -> ApiResult<Vec<PricingAccumulatorResponse>> {
        let rows = sqlx::query(
            "SELECT pricing_accumulator_id,price_version_id,cycle_key, \
             accumulated_converted_item_units,converted_blocks,version FROM pricing_accumulators \
             WHERE customer_id=$1 AND item_id=$2 AND ($3::uuid IS NULL OR price_version_id=$3) \
             ORDER BY created_at,pricing_accumulator_id",
        )
        .bind(workspace_id)
        .bind(item_id)
        .bind(price_id)
        .fetch_all(&self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|row| PricingAccumulatorResponse {
                pricing_accumulator_id: row.get("pricing_accumulator_id"),
                price_version_id: row.get("price_version_id"),
                cycle_key: row.get("cycle_key"),
                accumulated_converted_item_units: boundary(
                    row.get("accumulated_converted_item_units"),
                ),
                converted_blocks: row.get("converted_blocks"),
                version: row.get("version"),
            })
            .collect())
    }

    pub async fn reconcile_item_usage(
        &self,
        workspace_id: Uuid,
        item_id: Uuid,
    ) -> ApiResult<UsageReconciliationResponse> {
        let row = sqlx::query(
            "SELECT iw.total_received_item_units,iw.total_converted_item_units,iw.pending_item_units, \
             COALESCE((SELECT sum(e.received_item_units)::bigint FROM item_wallet_entries e \
               WHERE e.item_wallet_id=iw.wallet_id),0) statement_received, \
             COALESCE((SELECT sum(b.unit_block_size)::bigint FROM billing_blocks b \
               WHERE b.item_wallet_id=iw.wallet_id),0) block_converted, \
             COALESCE((SELECT sum(a.allocated_credit_units)::bigint FROM credit_lot_allocations a \
               JOIN debits d ON d.debit_id=a.debit_id JOIN usage_events u ON u.usage_event_id=d.usage_event_id \
               WHERE u.item_wallet_id=iw.wallet_id),0) allocated, \
             COALESCE((SELECT sum(d.debited_credit_units)::bigint FROM debits d JOIN usage_events u \
               ON u.usage_event_id=d.usage_event_id WHERE u.item_wallet_id=iw.wallet_id),0) debited \
             FROM item_wallets iw JOIN wallets w ON w.wallet_id=iw.wallet_id \
             WHERE w.customer_id=$1 AND w.item_id=$2",
        )
        .bind(workspace_id)
        .bind(item_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing_item_wallet(workspace_id, item_id))?;
        let received: i64 = row.get("total_received_item_units");
        let converted: i64 = row.get("total_converted_item_units");
        let pending: i64 = row.get("pending_item_units");
        let statement: i64 = row.get("statement_received");
        let blocks: i64 = row.get("block_converted");
        let allocated: i64 = row.get("allocated");
        let debited: i64 = row.get("debited");
        Ok(UsageReconciliationResponse {
            customer_id: workspace_id,
            item_id,
            meter_received_item_units: boundary(received),
            statement_received_item_units: boundary(statement),
            meter_converted_item_units: boundary(converted),
            block_converted_item_units: boundary(blocks),
            meter_pending_item_units: boundary(pending),
            allocated_credit_units: CreditUnits::new(allocated),
            debited_credit_units: CreditUnits::new(debited),
            consistent: received == statement
                && converted == blocks
                && received == converted + pending
                && allocated == debited,
        })
    }
}

fn eligibility_reason(
    product_active: bool,
    usable_plan: bool,
    entitled: bool,
    balance: Option<i64>,
) -> (bool, &'static str) {
    if !product_active || !usable_plan {
        return (false, "customer_plan_not_active");
    }
    if !entitled {
        return (false, "entitlement_not_granted");
    }
    if balance.is_none_or(|value| value < 0) {
        return (false, "insufficient_credit");
    }
    (true, "eligible")
}

fn billing_block_from_row(row: &sqlx::postgres::PgRow) -> ApiResult<BillingBlockResponse> {
    Ok(BillingBlockResponse {
        billing_block_id: row.get("billing_block_id"),
        global_block_sequence: row.get("global_block_sequence"),
        price_version_id: row.get("price_version_id"),
        price_block_ordinal: row.get("price_block_ordinal"),
        tier_position: row.get("tier_position"),
        cycle_key: row.get("cycle_key"),
        cycle_start: row.get("cycle_start"),
        cycle_end: row.get("cycle_end"),
        accumulated_units_before: boundary(row.get("accumulated_units_before")),
        accumulated_units_after: boundary(row.get("accumulated_units_after")),
        unit_block_size: ItemUnits::positive(row.get("unit_block_size"))?,
        debited_credit_units: CreditUnits::new(row.get("debited_credit_units")),
        unit_offset_start: boundary(row.get("unit_offset_start")),
        unit_offset_end: boundary(row.get("unit_offset_end")),
    })
}

fn renewal_status(commercial_status: &str) -> &'static str {
    if commercial_status == "PAST_DUE" {
        "RENEWAL_INACTIVE"
    } else {
        "CURRENT"
    }
}

fn price_cycle_key(row: &sqlx::postgres::PgRow) -> ApiResult<String> {
    let anchor: Option<DateTime<Utc>> = row.get("accumulation_anchor_at");
    let Some(anchor) = anchor else {
        return Ok("lifetime".to_string());
    };
    let rule: Option<String> = row.get("accumulation_recurrence_rule");
    let rule = rule.ok_or_else(|| ApiError::unexpected("tiered price cycle rule is absent"))?;
    let (start, _) = pricing_cycle_bounds(anchor, &rule, Utc::now())?;
    Ok(start.to_rfc3339())
}

fn required_quote<T>(value: Option<T>, item_id: Uuid, field: &str) -> ApiResult<T> {
    value.ok_or_else(|| ApiError::unexpected(format!("item {item_id} next quote requires {field}")))
}

fn boundary(value: i64) -> ItemUnitBoundary {
    ItemUnitBoundary::non_negative(value)
        .expect("persisted item unit boundary must be non-negative")
}

fn missing(kind: &str, id: Uuid) -> ApiError {
    ApiError::not_found(
        "usage_resource_not_found",
        format!("{kind} {id} does not exist"),
    )
}

fn missing_item_wallet(workspace_id: Uuid, item_id: Uuid) -> ApiError {
    ApiError::service_unavailable(
        "wallet_not_provisioned",
        format!("workspace {workspace_id} has no item wallet for item {item_id}"),
    )
}
