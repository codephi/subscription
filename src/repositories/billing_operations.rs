use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::billing::{BillingOperationsResponse, UnmatchedPaymentCaseResponse},
    error::ApiResult,
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
    pub async fn list_unmatched_payments(
        &self,
        workspace_id: Uuid,
    ) -> ApiResult<Vec<UnmatchedPaymentCaseResponse>> {
        let rows = sqlx::query(
            "SELECT * FROM unmatched_payment_cases WHERE workspace_id=$1 \
             ORDER BY created_at,unmatched_payment_case_id",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool())
        .await?;
        Ok(rows.iter().map(unmatched_from_row).collect())
    }

    pub async fn billing_operations(&self) -> ApiResult<BillingOperationsResponse> {
        let row = sqlx::query(
            "SELECT \
             (SELECT count(*) FROM collection_requests WHERE status IN ('SCHEDULED','COLLECTING','PENDING_PAYMENT')) pending_collections, \
             (SELECT count(*) FROM billing_webhook_inbox WHERE failure_code IS NOT NULL) webhook_failures, \
             (SELECT count(*) FROM billing_webhook_inbox WHERE processed_at IS NULL) unprocessed_webhooks, \
             (SELECT count(*) FROM unmatched_payment_cases WHERE status='OPEN') open_unmatched_payments, \
             (SELECT count(*) FROM outbox_events WHERE delivered_at IS NULL AND dead_lettered_at IS NULL) outbox_backlog, \
             (SELECT count(*) FROM outbox_events WHERE dead_lettered_at IS NOT NULL) outbox_dead_letters",
        )
        .fetch_one(&self.pool())
        .await?;
        Ok(BillingOperationsResponse {
            pending_collections: row.get("pending_collections"),
            webhook_failures: row.get("webhook_failures"),
            unprocessed_webhooks: row.get("unprocessed_webhooks"),
            open_unmatched_payments: row.get("open_unmatched_payments"),
            outbox_backlog: row.get("outbox_backlog"),
            outbox_dead_letters: row.get("outbox_dead_letters"),
        })
    }

    pub async fn record_external_refund(
        &self,
        connection_id: Uuid,
        provider_event_id: &str,
        provider_payment_id: &str,
        amount_minor: i64,
        currency: &str,
        payload_sha256: &str,
    ) -> ApiResult<bool> {
        let result = sqlx::query(
            "INSERT INTO external_refund_observations (external_refund_observation_id, \
             billing_connection_id,workspace_id,provider,provider_event_id,provider_payment_id, \
             amount_minor,currency,payload_sha256) SELECT $1,bc.billing_connection_id,bc.workspace_id, \
             bc.provider,$3,$4,$5,$6,$7 FROM billing_connections bc \
             WHERE bc.billing_connection_id=$2 ON CONFLICT (provider,provider_event_id) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(connection_id)
        .bind(provider_event_id)
        .bind(provider_payment_id)
        .bind(amount_minor)
        .bind(currency)
        .bind(payload_sha256)
        .execute(&self.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

fn unmatched_from_row(row: &sqlx::postgres::PgRow) -> UnmatchedPaymentCaseResponse {
    UnmatchedPaymentCaseResponse {
        unmatched_payment_case_id: row.get("unmatched_payment_case_id"),
        workspace_id: row.get("workspace_id"),
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
