use chrono::{DateTime, Utc};
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnmatchedPaymentRecord {
    pub unmatched_payment_case_id: Uuid,
    pub account_id: Uuid,
    pub billing_connection_id: Uuid,
    pub provider: String,
    pub provider_event_id: String,
    pub provider_payment_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub reason: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

impl DatabaseRepository {
    /// Records a confirmed provider payment that has no local collection request.
    pub async fn record_unmatched_payment(
        &self,
        billing_connection_id: Uuid,
        webhook: &ConfirmedBillingWebhook,
    ) -> ApiResult<UnmatchedPaymentRecord> {
        let mut transaction = self.pool().begin().await?;
        let inserted_event = insert_webhook_inbox(&mut transaction, webhook).await?;
        if !inserted_event {
            validate_duplicate_payload(&mut transaction, webhook).await?;
            let case =
                load_duplicate_case(&mut transaction, billing_connection_id, webhook).await?;
            transaction.commit().await?;
            return Ok(case);
        }
        let case = open_unmatched_case(&mut transaction, billing_connection_id, webhook).await?;
        transaction.commit().await?;
        Ok(case)
    }
}

async fn load_duplicate_case(
    transaction: &mut Transaction<'_, Postgres>,
    connection_id: Uuid,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<UnmatchedPaymentRecord> {
    let account_id = lock_billing_connection(transaction, connection_id, webhook).await?;
    let case = find_case_by_event(transaction, webhook).await?;
    validate_existing_case(&case, account_id, connection_id, webhook)?;
    Ok(case)
}

async fn open_unmatched_case(
    transaction: &mut Transaction<'_, Postgres>,
    connection_id: Uuid,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<UnmatchedPaymentRecord> {
    let account_id = lock_billing_connection(transaction, connection_id, webhook).await?;
    ensure_collection_is_missing(transaction, webhook.collection_request_id).await?;
    let inserted = insert_case(transaction, account_id, connection_id, webhook).await?;
    let case = find_case_by_payment(transaction, webhook).await?;
    validate_existing_case(&case, account_id, connection_id, webhook)?;
    if inserted {
        insert_opened_history(transaction, &case, webhook).await?;
        insert_unmatched_outbox(transaction, &case).await?;
    }
    mark_webhook(
        transaction,
        webhook,
        "UNMATCHED",
        Some("COLLECTION_REQUEST_NOT_FOUND"),
    )
    .await?;
    Ok(case)
}

async fn lock_billing_connection(
    transaction: &mut Transaction<'_, Postgres>,
    connection_id: Uuid,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<Uuid> {
    let row = sqlx::query(
        "SELECT account_id,provider,status FROM billing_connections \
         WHERE billing_connection_id=$1 FOR SHARE",
    )
    .bind(connection_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| missing_connection(connection_id))?;
    if row.get::<String, _>("provider") == webhook.provider
        && row.get::<String, _>("status") == "ACTIVE"
    {
        return Ok(row.get("account_id"));
    }
    Err(ApiError::conflict(
        "billing_connection_mismatch",
        format!(
            "connection {connection_id} must be ACTIVE for provider {:?}",
            webhook.provider
        ),
    ))
}

async fn ensure_collection_is_missing(
    transaction: &mut Transaction<'_, Postgres>,
    collection_request_id: Uuid,
) -> ApiResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM collection_requests WHERE collection_request_id=$1)",
    )
    .bind(collection_request_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !exists {
        return Ok(());
    }
    Err(ApiError::conflict(
        "collection_request_exists",
        format!("collection request {collection_request_id} must use normal confirmation"),
    ))
}

async fn insert_case(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    connection_id: Uuid,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<bool> {
    let evidence = serde_json::to_value(webhook).map_err(ApiError::serialization)?;
    let result = sqlx::query(
        "INSERT INTO unmatched_payment_cases (unmatched_payment_case_id,account_id, \
         billing_connection_id,provider,provider_event_id,provider_payment_id,amount_minor,currency, \
         reason,evidence,status) VALUES ($1,$2,$3,$4,$5,$6,$7,$8, \
         'COLLECTION_REQUEST_NOT_FOUND',$9,'OPEN') ON CONFLICT DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(account_id)
    .bind(connection_id)
    .bind(&webhook.provider)
    .bind(&webhook.provider_event_id)
    .bind(&webhook.provider_payment_id)
    .bind(webhook.amount_minor)
    .bind(&webhook.currency)
    .bind(evidence)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn find_case_by_event(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<UnmatchedPaymentRecord> {
    let row = sqlx::query(
        "SELECT * FROM unmatched_payment_cases WHERE provider=$1 AND provider_event_id=$2",
    )
    .bind(&webhook.provider)
    .bind(&webhook.provider_event_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| missing_unmatched_event(&webhook.provider_event_id))?;
    Ok(case_from_row(&row))
}

async fn find_case_by_payment(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<UnmatchedPaymentRecord> {
    let row = sqlx::query(
        "SELECT * FROM unmatched_payment_cases WHERE provider=$1 AND provider_payment_id=$2 FOR UPDATE",
    )
    .bind(&webhook.provider)
    .bind(&webhook.provider_payment_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(case_from_row(&row))
}

fn validate_existing_case(
    case: &UnmatchedPaymentRecord,
    account_id: Uuid,
    connection_id: Uuid,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<()> {
    if case.account_id == account_id
        && case.billing_connection_id == connection_id
        && case.amount_minor == webhook.amount_minor
        && case.currency == webhook.currency
    {
        return Ok(());
    }
    Err(ApiError::conflict(
        "unmatched_payment_mismatch",
        format!(
            "provider payment {:?} already has a different account, connection, amount, or currency",
            webhook.provider_payment_id
        ),
    ))
}

async fn insert_opened_history(
    transaction: &mut Transaction<'_, Postgres>,
    case: &UnmatchedPaymentRecord,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO unmatched_payment_case_events (unmatched_payment_case_event_id, \
         unmatched_payment_case_id,sequence,event_type,evidence) VALUES ($1,$2,1,'OPENED',$3)",
    )
    .bind(Uuid::new_v4())
    .bind(case.unmatched_payment_case_id)
    .bind(serde_json::to_value(webhook).map_err(ApiError::serialization)?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_unmatched_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    case: &UnmatchedPaymentRecord,
) -> ApiResult<()> {
    let event_id = Uuid::new_v4();
    let payload = json!({"billing_event_id":event_id,"event_type":"payment.unmatched",
        "schema_version":1,"occurred_at":case.created_at,"account_id":case.account_id,
        "unmatched_payment_case_id":case.unmatched_payment_case_id,
        "provider":case.provider,"provider_payment_id":case.provider_payment_id});
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         account_id,correlation_id,payload) VALUES \
         ($1,'payment.unmatched','unmatched_payment_case',$2,1,$3,$2,$4)",
    )
    .bind(event_id)
    .bind(case.unmatched_payment_case_id)
    .bind(case.account_id)
    .bind(payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn case_from_row(row: &sqlx::postgres::PgRow) -> UnmatchedPaymentRecord {
    UnmatchedPaymentRecord {
        unmatched_payment_case_id: row.get("unmatched_payment_case_id"),
        account_id: row.get("account_id"),
        billing_connection_id: row.get("billing_connection_id"),
        provider: row.get("provider"),
        provider_event_id: row.get("provider_event_id"),
        provider_payment_id: row.get("provider_payment_id"),
        amount_minor: row.get("amount_minor"),
        currency: row.get("currency"),
        reason: row.get("reason"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    }
}

fn missing_connection(connection_id: Uuid) -> ApiError {
    ApiError::not_found(
        "billing_connection_not_found",
        format!("billing connection {connection_id} does not exist"),
    )
}

fn missing_unmatched_event(provider_event_id: &str) -> ApiError {
    ApiError::conflict(
        "webhook_event_already_processed",
        format!("provider event {provider_event_id:?} is not an unmatched payment"),
    )
}
