use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::billing_investigation::{
        BillingRecordPageResponse, BillingRecordQuery, BillingRecordResponse,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

#[derive(Clone, Copy, Debug)]
pub enum BillingRecordKind {
    Collections,
    Attempts,
    Payments,
    Webhooks,
    Outbox,
    Unmatched,
}

impl BillingRecordKind {
    /// Resolve a public list slug; e.g. `BillingRecordKind::parse("outbox")`.
    pub fn parse(value: &str) -> ApiResult<Self> {
        match value {
            "collections" => Ok(Self::Collections),
            "attempts" => Ok(Self::Attempts),
            "payments" => Ok(Self::Payments),
            "webhooks" => Ok(Self::Webhooks),
            "outbox" => Ok(Self::Outbox),
            "unmatched" => Ok(Self::Unmatched),
            _ => Err(ApiError::unprocessable(
                "invalid_billing_record_kind",
                format!(
                    "kind {value} must be collections, attempts, payments, webhooks, outbox, or unmatched"
                ),
            )),
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::Collections => COLLECTIONS,
            Self::Attempts => ATTEMPTS,
            Self::Payments => PAYMENTS,
            Self::Webhooks => WEBHOOKS,
            Self::Outbox => OUTBOX,
            Self::Unmatched => UNMATCHED,
        }
    }
}

impl DatabaseRepository {
    /// Page billing evidence by stable UUID; e.g. `repo.list_billing_records(kind, query, 20).await`.
    pub async fn list_billing_records(
        &self,
        kind: BillingRecordKind,
        query: BillingRecordQuery,
        limit: i64,
    ) -> ApiResult<BillingRecordPageResponse> {
        let sql = format!(
            "{} SELECT * FROM records WHERE ($1::uuid IS NULL OR id > $1) AND ($2::uuid IS NULL OR workspace_id=$2) AND ($3::text IS NULL OR status=$3 OR ($3='ACTIVE' AND kind='collections' AND status IN ('SCHEDULED','COLLECTING','PENDING_PAYMENT')) OR ($3='UNPROCESSED' AND kind='webhooks' AND id IN (SELECT billing_webhook_inbox_id FROM billing_webhook_inbox WHERE processed_at IS NULL))) AND ($5::uuid IS NULL OR collection_request_id=$5) AND ($6::uuid IS NULL OR correlation_id=$6) ORDER BY id LIMIT $4",
            kind.source()
        );
        // Only SQL fragments from BillingRecordKind::source are interpolated; filters stay bound.
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(query.cursor)
            .bind(query.workspace_id)
            .bind(query.status)
            .bind(limit + 1)
            .bind(query.collection_request_id)
            .bind(query.correlation_id)
            .fetch_all(&self.pool())
            .await?;
        let has_more = rows.len() as i64 > limit;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(record_from_row)
            .collect::<Vec<_>>();
        let next_cursor = has_more.then(|| items.last().map(|item| item.id)).flatten();
        Ok(BillingRecordPageResponse { items, next_cursor })
    }

    /// Read billing evidence by ID; e.g. `repo.get_billing_record(kind, id).await`.
    pub async fn get_billing_record(
        &self,
        kind: BillingRecordKind,
        id: Uuid,
    ) -> ApiResult<BillingRecordResponse> {
        let sql = format!("{} SELECT * FROM records WHERE id=$1", kind.source());
        let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| {
                ApiError::not_found(
                    "billing_record_not_found",
                    format!("billing record {id} does not exist"),
                )
            })?;
        Ok(record_from_row(&row))
    }
}

fn record_from_row(row: &sqlx::postgres::PgRow) -> BillingRecordResponse {
    BillingRecordResponse {
        id: row.get("id"),
        kind: row.get("kind"),
        workspace_id: row.get("workspace_id"),
        status: row.get("status"),
        occurred_at: row.get("occurred_at"),
        collection_request_id: row.get("collection_request_id"),
        collection_attempt_id: row.get("collection_attempt_id"),
        billing_payment_id: row.get("billing_payment_id"),
        correlation_id: row.get("correlation_id"),
        provider_event_id: row.get("provider_event_id"),
        provider_payment_id: row.get("provider_payment_id"),
        event_type: row.get("event_type"),
        amount_minor: row.get("amount_minor"),
        currency: row.get("currency"),
        failure_code: row.get("failure_code"),
        detail: row.get("detail"),
        coupon_id: row.try_get("coupon_id").ok().flatten(),
        coupon_code: row.try_get("coupon_code").ok().flatten(),
        base_amount_minor: row.try_get("base_amount_minor").ok().flatten(),
        discount_amount_minor: row.try_get("discount_amount_minor").ok().flatten(),
        coupon_version: row.try_get("coupon_version").ok().flatten(),
    }
}

const COLLECTIONS: &str = "WITH records AS (SELECT cr.collection_request_id id, 'collections'::text kind, cr.workspace_id, cr.status, cr.created_at occurred_at, cr.collection_request_id, NULL::uuid collection_attempt_id, NULL::uuid billing_payment_id, cr.correlation_id, NULL::text provider_event_id, NULL::text provider_payment_id, cr.request_kind event_type, cr.amount_minor, cr.currency, cr.terminal_reason failure_code, cr.transaction_id detail, cr.coupon_id, cr.coupon_code, cr.base_amount_minor, cr.discount_amount_minor, cr.coupon_version FROM collection_requests cr)";
const ATTEMPTS: &str = "WITH records AS (SELECT ca.collection_attempt_id id, 'attempts'::text kind, cr.workspace_id, ca.status, ca.created_at occurred_at, ca.collection_request_id, ca.collection_attempt_id, bp.billing_payment_id, cr.correlation_id, NULL::text provider_event_id, bp.provider_payment_id, ca.connector event_type, bp.amount_minor, bp.currency, ca.failure_code, ca.provider_idempotency_key detail FROM collection_attempts ca JOIN collection_requests cr USING(collection_request_id) LEFT JOIN billing_payments bp USING(collection_attempt_id))";
const PAYMENTS: &str = "WITH records AS (SELECT bp.billing_payment_id id, 'payments'::text kind, cr.workspace_id, bp.state status, bp.created_at occurred_at, bp.collection_request_id, bp.collection_attempt_id, bp.billing_payment_id, cr.correlation_id, NULL::text provider_event_id, bp.provider_payment_id, bp.provider event_type, bp.amount_minor, bp.currency, bp.failure_code, NULL::text detail FROM billing_payments bp JOIN collection_requests cr USING(collection_request_id))";
const WEBHOOKS: &str = "WITH records AS (SELECT wh.billing_webhook_inbox_id id, 'webhooks'::text kind, COALESCE(cr.workspace_id, uc.workspace_id) workspace_id, CASE WHEN wh.failure_code IS NOT NULL THEN 'FAILURE' WHEN wh.processed_at IS NULL THEN 'UNPROCESSED' ELSE COALESCE(wh.result,'PROCESSED') END status, wh.received_at occurred_at, cr.collection_request_id, NULL::uuid collection_attempt_id, NULL::uuid billing_payment_id, cr.correlation_id, wh.provider_event_id, wh.payload->>'provider_payment_id' provider_payment_id, wh.event_type, NULL::bigint amount_minor, NULL::text currency, wh.failure_code, wh.provider detail FROM billing_webhook_inbox wh LEFT JOIN collection_requests cr ON cr.collection_request_id::text=wh.payload->>'collection_request_id' LEFT JOIN unmatched_payment_cases uc ON uc.provider=wh.provider AND uc.provider_event_id=wh.provider_event_id)";
const OUTBOX: &str = "WITH records AS (SELECT ob.event_id id, 'outbox'::text kind, ob.workspace_id, CASE WHEN ob.dead_lettered_at IS NOT NULL THEN 'DEAD_LETTER' WHEN ob.delivered_at IS NOT NULL THEN 'DELIVERED' ELSE 'PENDING' END status, ob.occurred_at, CASE WHEN ob.aggregate_type='collection_request' THEN ob.aggregate_id ELSE NULL END collection_request_id, NULL::uuid collection_attempt_id, NULL::uuid billing_payment_id, ob.correlation_id, NULL::text provider_event_id, NULL::text provider_payment_id, ob.event_type, NULL::bigint amount_minor, NULL::text currency, NULL::text failure_code, ob.last_error detail FROM outbox_events ob)";
const UNMATCHED: &str = "WITH records AS (SELECT uc.unmatched_payment_case_id id, 'unmatched'::text kind, uc.workspace_id, uc.status, uc.created_at occurred_at, NULL::uuid collection_request_id, NULL::uuid collection_attempt_id, NULL::uuid billing_payment_id, NULL::uuid correlation_id, uc.provider_event_id, uc.provider_payment_id, uc.provider event_type, uc.amount_minor, uc.currency, NULL::text failure_code, uc.reason detail FROM unmatched_payment_cases uc)";
