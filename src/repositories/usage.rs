use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{
        units::{CreditUnits, ItemUnitBoundary, ItemUnits},
        usage::{CreateUsageEventRequest, UsageAllocationResponse, UsageEventResponse},
    },
    error::{ApiError, ApiResult},
    repositories::{
        credit_writes::complete_reservations,
        credits::{reserve_idempotency, reserve_transaction},
        database::DatabaseRepository,
        usage_models::{Conversion, DebitResult, LockedMeter},
        usage_pricing::calculate_usage,
        usage_writes::{apply_usage_debit, persist_usage},
    },
};

impl DatabaseRepository {
    pub async fn insert_usage_event(
        &self,
        workspace_id: Uuid,
        idempotency_key: &str,
        request_hash: &str,
        request: &CreateUsageEventRequest,
    ) -> ApiResult<UsageEventResponse> {
        let mut transaction = self.pool().begin().await?;
        lock_catalog(&mut transaction, request.product_id, request.item_id).await?;
        ensure_workspace_active(&mut transaction, workspace_id).await?;
        let meter = lock_item_meter(&mut transaction, workspace_id, request.item_id).await?;
        let accepted_at = database_clock(&mut transaction).await?;
        ensure_entitlement(
            &mut transaction,
            workspace_id,
            request.product_id,
            accepted_at,
        )
        .await?;
        reserve_usage_keys(
            &mut transaction,
            workspace_id,
            idempotency_key,
            request_hash,
            &request.transaction_id,
        )
        .await?;
        let conversion = calculate_usage(&mut transaction, request, &meter, accepted_at).await?;
        let usage_event_id = Uuid::new_v4();
        let item_entry_id = Uuid::new_v4();
        let debit = create_debit_if_needed(
            &mut transaction,
            workspace_id,
            usage_event_id,
            &meter,
            &conversion,
            request,
            accepted_at,
        )
        .await?;
        persist_usage(
            &mut transaction,
            usage_event_id,
            item_entry_id,
            workspace_id,
            request,
            accepted_at,
            &meter,
            &conversion,
            debit.as_ref(),
        )
        .await?;
        complete_reservations(
            &mut transaction,
            workspace_id,
            idempotency_key,
            &request.transaction_id,
            usage_event_id,
        )
        .await?;
        transaction.commit().await?;
        build_response(
            usage_event_id,
            item_entry_id,
            request,
            accepted_at,
            &meter,
            &conversion,
            debit,
        )
    }
}

async fn lock_catalog(
    transaction: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
    item_id: Uuid,
) -> ApiResult<()> {
    let usage_model: Option<String> = sqlx::query_scalar(
        "SELECT p.usage_model FROM products p JOIN items i ON i.product_id=p.product_id \
         WHERE p.product_id=$1 AND i.item_id=$2 AND p.status='ACTIVE' AND i.status='ACTIVE' \
         FOR SHARE OF p,i",
    )
    .bind(product_id)
    .bind(item_id)
    .fetch_optional(&mut **transaction)
    .await?;
    match usage_model.as_deref() {
        Some("CREDIT_METERED") => Ok(()),
        Some(_) => Err(ApiError::conflict(
            "product_not_metered",
            format!("product {product_id} does not accept usage events"),
        )),
        None => Err(ApiError::not_found(
            "usage_catalog_not_found",
            format!("active product {product_id} must own active item {item_id}"),
        )),
    }
}

async fn ensure_workspace_active(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> ApiResult<()> {
    let row = sqlx::query(
        "SELECT operational_status,EXISTS(SELECT 1 FROM integration_inbox_quarantine q \
         JOIN integration_inbox i ON i.event_id=q.event_id WHERE i.workspace_id=$1 \
         AND q.replayed_at IS NULL) has_gap FROM workspace_projections WHERE workspace_id=$1 FOR SHARE",
    )
    .bind(workspace_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        ApiError::not_found(
            "workspace_not_found",
            format!("workspace {workspace_id} does not exist"),
        )
    })?;
    let status: String = row.get("operational_status");
    if status == "ACTIVE" && !row.get::<bool, _>("has_gap") {
        return Ok(());
    }
    Err(ApiError::conflict(
        "workspace_not_operational",
        format!("workspace {workspace_id} must be ACTIVE without event gaps, found {status}"),
    ))
}

async fn lock_item_meter(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    item_id: Uuid,
) -> ApiResult<LockedMeter> {
    let row = sqlx::query(
        "SELECT w.wallet_id,w.parent_customer_wallet_id,es.status,iw.total_received_item_units, \
         iw.total_converted_item_units,iw.total_converted_blocks,iw.pending_item_units, \
         iw.pending_price_version_id,iw.pending_tier_position,iw.pending_unit_block_size, \
         iw.pending_credit_units,iw.version FROM wallets w JOIN item_wallets iw ON iw.wallet_id=w.wallet_id \
         JOIN wallet_effective_states es ON es.wallet_id=w.wallet_id JOIN catalog_scope_current sc ON sc.singleton \
         JOIN wallet_provisioning vp ON vp.customer_id=w.customer_id AND vp.scope_version=sc.scope_version \
         WHERE w.customer_id=$1 AND w.item_id=$2 AND vp.status='ACTIVE' \
         AND vp.expected_item_wallets=vp.materialized_item_wallets FOR UPDATE OF iw,es",
    )
    .bind(workspace_id)
    .bind(item_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        ApiError::service_unavailable(
            "wallet_not_provisioned",
            format!("workspace {workspace_id} requires a materialized item wallet for item {item_id}"),
        )
    })?;
    let status: String = row.get("status");
    if status != "ACTIVE" {
        return Err(ApiError::conflict(
            "item_wallet_not_active",
            format!("item wallet for workspace {workspace_id} and item {item_id} is {status}"),
        ));
    }
    Ok(meter_from_row(&row))
}

fn meter_from_row(row: &sqlx::postgres::PgRow) -> LockedMeter {
    LockedMeter {
        wallet_id: row.get("wallet_id"),
        customer_wallet_id: row.get("parent_customer_wallet_id"),
        total_received: row.get("total_received_item_units"),
        total_converted: row.get("total_converted_item_units"),
        total_blocks: row.get("total_converted_blocks"),
        pending: row.get("pending_item_units"),
        pending_price_id: row.get("pending_price_version_id"),
        pending_tier_position: row.get("pending_tier_position"),
        pending_block_size: row.get("pending_unit_block_size"),
        pending_credit_units: row.get("pending_credit_units"),
        version: row.get("version"),
    }
}

async fn ensure_entitlement(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    product_id: Uuid,
    accepted_at: DateTime<Utc>,
) -> ApiResult<()> {
    let usable_plan: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM customer_plans WHERE customer_id=$1 \
         AND commercial_status IN ('ACTIVE','ACTIVE_PAID','PAST_DUE') AND activation_status='ACTIVATED')",
    )
    .bind(workspace_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !usable_plan {
        return Err(ApiError::conflict(
            "customer_plan_not_active",
            format!("workspace {workspace_id} has no usable activated customer plan"),
        ));
    }
    let entitled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM customer_plans c JOIN customer_plan_entitlements e \
         ON e.customer_plan_id=c.customer_plan_id WHERE c.customer_id=$1 \
         AND c.commercial_status IN ('ACTIVE','ACTIVE_PAID','PAST_DUE') \
         AND c.activation_status='ACTIVATED' AND e.product_id=$2 AND e.effective_from<=$3 \
         AND (e.effective_until IS NULL OR e.effective_until>$3))",
    )
    .bind(workspace_id)
    .bind(product_id)
    .bind(accepted_at)
    .fetch_one(&mut **transaction)
    .await?;
    if entitled {
        return Ok(());
    }
    Err(ApiError::forbidden(
        "entitlement_not_granted",
        format!("workspace {workspace_id} has no effective entitlement for product {product_id}"),
    ))
}

async fn database_clock(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **transaction)
        .await
}

async fn reserve_usage_keys(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    idempotency_key: &str,
    request_hash: &str,
    transaction_id: &str,
) -> ApiResult<()> {
    reserve_idempotency(
        transaction,
        workspace_id,
        idempotency_key,
        request_hash,
        "USAGE",
    )
    .await?;
    reserve_transaction(transaction, workspace_id, transaction_id, "USAGE").await
}

#[allow(clippy::too_many_arguments)]
async fn create_debit_if_needed(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    usage_event_id: Uuid,
    meter: &LockedMeter,
    conversion: &Conversion,
    request: &CreateUsageEventRequest,
    accepted_at: DateTime<Utc>,
) -> ApiResult<Option<DebitResult>> {
    if conversion.debited_credits == 0 {
        return Ok(None);
    }
    apply_usage_debit(
        transaction,
        workspace_id,
        usage_event_id,
        meter,
        conversion,
        request,
        accepted_at,
    )
    .await
    .map(Some)
}

fn build_response(
    usage_id: Uuid,
    item_entry_id: Uuid,
    request: &CreateUsageEventRequest,
    accepted_at: DateTime<Utc>,
    meter: &LockedMeter,
    conversion: &Conversion,
    debit: Option<DebitResult>,
) -> ApiResult<UsageEventResponse> {
    let allocations = conversion
        .blocks
        .iter()
        .map(|block| UsageAllocationResponse {
            price_version_id: block.price_id,
            pricing_model: block.model.clone(),
            tier_position: block.tier_position,
            unit_block_size: ItemUnits::positive(block.block_size).expect("stored positive block"),
            cycle_key: block.cycle.key.clone(),
            accumulated_units_before: boundary(block.accumulated_before),
            accumulated_units_after: boundary(block.accumulated_after),
            converted_blocks: 1,
            debited_credit_units: CreditUnits::new(block.credit_units),
        })
        .collect();
    Ok(UsageEventResponse {
        usage_event_id: usage_id,
        item_wallet_id: meter.wallet_id,
        item_wallet_entry_id: item_entry_id,
        debit_id: debit.as_ref().map(|value| value.debit_id),
        customer_wallet_entry_id: debit.as_ref().map(|value| value.entry_id),
        transaction_id: request.transaction_id.clone(),
        received_item_units: request.item_units,
        pending_item_units_before: boundary(meter.pending),
        converted_item_units: boundary(conversion.converted_units),
        converted_blocks: i64::try_from(conversion.blocks.len()).unwrap_or(i64::MAX),
        pending_item_units_after: boundary(conversion.pending_after),
        allocations,
        debited_credit_units: CreditUnits::new(conversion.debited_credits),
        balance_before_credit_units: debit
            .as_ref()
            .map(|value| CreditUnits::new(value.balance_before)),
        balance_after_credit_units: debit
            .as_ref()
            .map(|value| CreditUnits::new(value.balance_after)),
        billing_status: if debit.is_some() {
            "DEBITED"
        } else {
            "PENDING_BLOCK"
        }
        .to_string(),
        product_eligible_after: true,
        accepted_at,
    })
}

fn boundary(value: i64) -> ItemUnitBoundary {
    ItemUnitBoundary::non_negative(value)
        .expect("persisted item unit boundary must be non-negative")
}
