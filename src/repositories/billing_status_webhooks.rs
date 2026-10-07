use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    repositories::{
        billing_confirmation::ConfirmedBillingWebhook,
        billing_webhooks::{insert_webhook_inbox, mark_webhook, validate_duplicate_payload},
        database::DatabaseRepository,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaymentStatusWebhookResult {
    Applied,
    Duplicate,
    Rejected,
}

impl DatabaseRepository {
    pub async fn apply_payment_status_webhook(
        &self,
        webhook: &ConfirmedBillingWebhook,
    ) -> ApiResult<PaymentStatusWebhookResult> {
        let mut transaction = self.pool().begin().await?;
        if !insert_webhook_inbox(&mut transaction, webhook).await? {
            validate_duplicate_payload(&mut transaction, webhook).await?;
            transaction.commit().await?;
            return Ok(PaymentStatusWebhookResult::Duplicate);
        }
        let row = lock_payment(&mut transaction, webhook).await?;
        validate_payment(webhook, &row)?;
        let request_status: String = row.get("request_status");
        if matches!(
            request_status.as_str(),
            "PAID" | "EXPIRED" | "EXHAUSTED" | "CANCELED" | "UNMATCHED"
        ) {
            mark_webhook(&mut transaction, webhook, "REJECTED", None).await?;
            transaction.commit().await?;
            return Ok(PaymentStatusWebhookResult::Rejected);
        }
        let (payment_state, attempt_status, request_status, event_type) = normalized(webhook)?;
        sqlx::query("UPDATE billing_payments SET state=$2,failure_code=$3,provider_payment_id=COALESCE(provider_payment_id,$4) WHERE billing_payment_id=$1")
            .bind(row.get::<Uuid, _>("billing_payment_id")).bind(payment_state)
            .bind((payment_state == "FAILED").then_some("PROVIDER_FAILURE"))
            .bind(&webhook.provider_payment_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE collection_attempts SET status=$2,finished_at=CASE WHEN $2='FAILED' THEN $3 ELSE NULL END, \
             failure_code=CASE WHEN $2='FAILED' THEN 'PROVIDER_FAILURE' ELSE NULL END WHERE collection_attempt_id=$1")
            .bind(row.get::<Uuid, _>("collection_attempt_id")).bind(attempt_status)
            .bind(webhook.occurred_at).execute(&mut *transaction).await?;
        sqlx::query(
            "UPDATE collection_requests SET status=$2,terminal_reason=CASE WHEN $2='EXHAUSTED' \
             THEN 'PROVIDER_FAILURE' ELSE terminal_reason END WHERE collection_request_id=$1",
        )
        .bind(webhook.collection_request_id)
        .bind(request_status)
        .execute(&mut *transaction)
        .await?;
        if request_status == "EXHAUSTED" {
            apply_definitive_failure(&mut transaction, webhook.collection_request_id).await?;
        }
        insert_status_event(&mut transaction, webhook, row.get("account_id"), event_type).await?;
        mark_webhook(&mut transaction, webhook, "APPLIED", None).await?;
        transaction.commit().await?;
        Ok(PaymentStatusWebhookResult::Applied)
    }
}

async fn lock_payment(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<sqlx::postgres::PgRow> {
    sqlx::query("SELECT cr.account_id,cr.status request_status,bp.billing_payment_id,bp.provider, \
         bp.provider_payment_id,bp.amount_minor,bp.currency,ca.collection_attempt_id \
         FROM collection_requests cr JOIN billing_payments bp USING(collection_request_id) \
         JOIN collection_attempts ca USING(collection_attempt_id) WHERE cr.collection_request_id=$1 \
         FOR UPDATE OF cr,bp,ca")
        .bind(webhook.collection_request_id).fetch_optional(&mut **transaction).await?
        .ok_or_else(|| ApiError::not_found("collection_request_not_found", format!(
            "collection request {} does not exist", webhook.collection_request_id
        )))
}

fn validate_payment(
    webhook: &ConfirmedBillingWebhook,
    row: &sqlx::postgres::PgRow,
) -> ApiResult<()> {
    let stored_payment: Option<String> = row.get("provider_payment_id");
    if row.get::<String, _>("provider") == webhook.provider
        && stored_payment
            .as_deref()
            .is_none_or(|id| id == webhook.provider_payment_id)
        && row.get::<i64, _>("amount_minor") == webhook.amount_minor
        && row.get::<String, _>("currency") == webhook.currency
    {
        return Ok(());
    }
    Err(ApiError::conflict(
        "billing_confirmation_mismatch",
        format!(
            "provider event {} does not match collection {} payment snapshot",
            webhook.provider_event_id, webhook.collection_request_id
        ),
    ))
}

fn normalized(
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<(&'static str, &'static str, &'static str, &'static str)> {
    match webhook.event_type.as_str() {
        "payment.requires_action" => Ok((
            "REQUIRES_ACTION",
            "REQUIRES_ACTION",
            "PENDING_PAYMENT",
            "payment.requires_action",
        )),
        "payment.failed" => Ok(("FAILED", "FAILED", "EXHAUSTED", "payment.failed")),
        other => Err(ApiError::unprocessable(
            "unsupported_billing_event",
            format!(
                "billing event type {other:?} must be payment.requires_action or payment.failed"
            ),
        )),
    }
}

async fn apply_definitive_failure(
    transaction: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE customer_plans cp SET commercial_status=CASE cr.request_kind \
         WHEN 'INITIAL' THEN 'CANCELED' WHEN 'RENEWAL' THEN 'PAST_DUE' ELSE cp.commercial_status END, \
         activation_status=CASE WHEN cr.request_kind='INITIAL' THEN 'FAILED' ELSE cp.activation_status END, \
         renewal_status=CASE WHEN cr.request_kind IN ('INITIAL','RENEWAL') THEN 'RENEWAL_INACTIVE' ELSE cp.renewal_status END, \
         ended_at=CASE WHEN cr.request_kind='INITIAL' THEN clock_timestamp() ELSE cp.ended_at END, \
         end_reason=CASE WHEN cr.request_kind='INITIAL' THEN 'INITIAL_PAYMENT_FAILED' ELSE cp.end_reason END, \
         version=CASE WHEN cr.request_kind IN ('INITIAL','RENEWAL') THEN cp.version+1 ELSE cp.version END \
         FROM collection_requests cr WHERE cr.collection_request_id=$1 AND cr.customer_plan_id=cp.customer_plan_id")
        .bind(request_id).execute(&mut **transaction).await?;
    Ok(())
}

async fn insert_status_event(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
    account_id: Uuid,
    event_type: &str,
) -> ApiResult<()> {
    let event_id = Uuid::new_v4();
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(aggregate_sequence),0)+1 FROM outbox_events \
         WHERE aggregate_type='collection_request' AND aggregate_id=$1",
    )
    .bind(webhook.collection_request_id)
    .fetch_one(&mut **transaction)
    .await?;
    let correlation_id: Uuid = sqlx::query_scalar(
        "SELECT correlation_id FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(webhook.collection_request_id)
    .fetch_one(&mut **transaction)
    .await?;
    let payload = json!({"billing_event_id":event_id,"event_type":event_type,"schema_version":1,
        "occurred_at":webhook.occurred_at,"account_id":account_id,"correlation_id":correlation_id,
        "collection_request_id":webhook.collection_request_id,"provider_payment_id":webhook.provider_payment_id});
    sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         account_id,correlation_id,payload) VALUES ($1,$2,'collection_request',$3,$4,$5,$6,$7)")
        .bind(event_id).bind(event_type).bind(webhook.collection_request_id).bind(sequence)
        .bind(account_id).bind(correlation_id).bind(payload).execute(&mut **transaction).await?;
    Ok(())
}
