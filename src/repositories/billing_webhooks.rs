use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    repositories::billing_confirmation::ConfirmedBillingWebhook,
};

pub(crate) async fn insert_webhook_inbox(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<bool> {
    let result = sqlx::query(
        "INSERT INTO billing_webhook_inbox (billing_webhook_inbox_id,provider,provider_event_id, \
         event_type,payload_sha256,payload) VALUES ($1,$2,$3,$4,$5,$6) \
         ON CONFLICT (provider,provider_event_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&webhook.provider)
    .bind(&webhook.provider_event_id)
    .bind(&webhook.event_type)
    .bind(&webhook.payload_sha256)
    .bind(serde_json::to_value(webhook).map_err(ApiError::serialization)?)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn validate_duplicate_payload(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<()> {
    let stored_hash: String = sqlx::query_scalar(
        "SELECT payload_sha256 FROM billing_webhook_inbox WHERE provider=$1 AND provider_event_id=$2",
    )
    .bind(&webhook.provider)
    .bind(&webhook.provider_event_id)
    .fetch_one(&mut **transaction)
    .await?;
    if stored_hash == webhook.payload_sha256 {
        return Ok(());
    }
    Err(ApiError::conflict(
        "webhook_event_payload_mismatch",
        format!(
            "provider event {} already has payload hash {stored_hash}, not {}",
            webhook.provider_event_id, webhook.payload_sha256
        ),
    ))
}

pub(crate) async fn mark_webhook(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
    result: &str,
    failure_code: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE billing_webhook_inbox SET processed_at=clock_timestamp(),result=$3,failure_code=$4 \
         WHERE provider=$1 AND provider_event_id=$2",
    )
    .bind(&webhook.provider)
    .bind(&webhook.provider_event_id)
    .bind(result)
    .bind(failure_code)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
