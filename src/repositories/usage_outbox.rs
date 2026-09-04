use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    dto::events::DomainEventEnvelope,
    error::{ApiError, ApiResult},
    repositories::usage_models::{Conversion, DebitResult, LockedMeter},
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_usage_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    usage_id: Uuid,
    meter: &LockedMeter,
    conversion: &Conversion,
    debit: Option<&DebitResult>,
    occurred_at: DateTime<Utc>,
    received_item_units: i64,
) -> ApiResult<()> {
    persist_event(
        transaction,
        domain_event(
            "usage.recorded",
            "item_wallet",
            meter.wallet_id,
            meter.version + 1,
            workspace_id,
            occurred_at,
            json!({
                "usage_event_id": usage_id,
                "received_item_units": received_item_units.to_string(),
                "converted_item_units": conversion.converted_units.to_string(),
                "pending_item_units": conversion.pending_after.to_string()
            }),
        ),
    )
    .await?;
    if let Some(debit) = debit {
        persist_debit_event(
            transaction,
            workspace_id,
            usage_id,
            meter,
            conversion,
            debit,
            occurred_at,
        )
        .await?;
    }
    Ok(())
}

async fn persist_debit_event(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    usage_id: Uuid,
    meter: &LockedMeter,
    conversion: &Conversion,
    debit: &DebitResult,
    occurred_at: DateTime<Utc>,
) -> ApiResult<()> {
    persist_event(
        transaction,
        domain_event(
            "wallet.debited",
            "customer_wallet",
            meter.customer_wallet_id,
            debit.entry_sequence,
            workspace_id,
            occurred_at,
            json!({
                "usage_event_id": usage_id,
                "debit_id": debit.debit_id,
                "debited_credit_units": conversion.debited_credits.to_string()
            }),
        ),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
fn domain_event(
    event_type: &str,
    aggregate_type: &str,
    aggregate_id: Uuid,
    sequence: i64,
    workspace_id: Uuid,
    occurred_at: DateTime<Utc>,
    payload: Value,
) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::new_v4(),
        event_type: event_type.to_string(),
        schema_version: 1,
        aggregate_type: aggregate_type.to_string(),
        aggregate_id,
        sequence,
        occurred_at,
        workspace_id,
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        payload,
    }
}

async fn persist_event(
    transaction: &mut Transaction<'_, Postgres>,
    event: DomainEventEnvelope,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id, \
         aggregate_sequence,workspace_id,correlation_id,causation_id,payload,occurred_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(event.event_id)
    .bind(&event.event_type)
    .bind(&event.aggregate_type)
    .bind(event.aggregate_id)
    .bind(event.sequence)
    .bind(event.workspace_id)
    .bind(event.correlation_id)
    .bind(event.causation_id)
    .bind(serde_json::to_value(&event).map_err(ApiError::serialization)?)
    .bind(event.occurred_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
