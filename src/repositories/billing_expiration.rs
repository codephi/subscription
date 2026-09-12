use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{error::ApiResult, repositories::database::DatabaseRepository};

#[derive(Debug, Default, Eq, PartialEq, Serialize)]
pub struct CollectionExpirationSummary {
    pub expired_requests: i64,
    pub canceled_initial_plans: i64,
    pub past_due_renewals: i64,
}

impl DatabaseRepository {
    /// Expires every due commercial collection visible at `as_of`.
    ///
    /// Call this from the Billing scheduler with a stable clock value so every
    /// claimed request is evaluated against the same commercial deadline.
    pub async fn expire_due_collections(
        &self,
        as_of: DateTime<Utc>,
    ) -> ApiResult<CollectionExpirationSummary> {
        let mut summary = CollectionExpirationSummary::default();
        while let Some(effect) = self.expire_next_collection(as_of).await? {
            summary.expired_requests += 1;
            summary.canceled_initial_plans += i64::from(effect.canceled_initial);
            summary.past_due_renewals += i64::from(effect.past_due_renewal);
        }
        Ok(summary)
    }

    async fn expire_next_collection(
        &self,
        as_of: DateTime<Utc>,
    ) -> ApiResult<Option<ExpirationEffect>> {
        let mut transaction = self.pool().begin().await?;
        let Some(request) = lock_due_request(&mut transaction, as_of).await? else {
            transaction.commit().await?;
            return Ok(None);
        };
        let effect = expire_request(&mut transaction, &request, as_of).await?;
        transaction.commit().await?;
        Ok(Some(effect))
    }
}

struct DueCollection {
    request_id: Uuid,
    workspace_id: Uuid,
    customer_plan_id: Option<Uuid>,
    request_kind: String,
    correlation_id: Uuid,
}

struct ExpirationEffect {
    canceled_initial: bool,
    past_due_renewal: bool,
}

async fn lock_due_request(
    transaction: &mut Transaction<'_, Postgres>,
    as_of: DateTime<Utc>,
) -> Result<Option<DueCollection>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT collection_request_id,workspace_id,customer_plan_id,request_kind,correlation_id \
         FROM collection_requests WHERE status IN ('SCHEDULED','COLLECTING','PENDING_PAYMENT') \
         AND payment_expires_at<=$1 ORDER BY payment_expires_at,collection_request_id \
         LIMIT 1 FOR UPDATE SKIP LOCKED",
    )
    .bind(as_of)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.map(|row| DueCollection {
        request_id: row.get("collection_request_id"),
        workspace_id: row.get("workspace_id"),
        customer_plan_id: row.get("customer_plan_id"),
        request_kind: row.get("request_kind"),
        correlation_id: row.get("correlation_id"),
    }))
}

async fn expire_request(
    transaction: &mut Transaction<'_, Postgres>,
    request: &DueCollection,
    as_of: DateTime<Utc>,
) -> ApiResult<ExpirationEffect> {
    sqlx::query(
        "UPDATE collection_requests SET status='EXPIRED',terminal_reason='PAYMENT_WINDOW_EXPIRED' \
         WHERE collection_request_id=$1",
    )
    .bind(request.request_id)
    .execute(&mut **transaction)
    .await?;
    let canceled_initial = update_initial_plan(transaction, request, as_of).await?;
    let past_due_renewal = update_renewal_plan(transaction, request).await?;
    insert_expiration_event(transaction, request, as_of).await?;
    Ok(ExpirationEffect {
        canceled_initial,
        past_due_renewal,
    })
}

async fn update_initial_plan(
    transaction: &mut Transaction<'_, Postgres>,
    request: &DueCollection,
    as_of: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    if request.request_kind != "INITIAL" {
        return Ok(false);
    }
    let result = sqlx::query(
        "UPDATE customer_plans SET commercial_status='CANCELED',activation_status='FAILED', \
         renewal_status='RENEWAL_INACTIVE',ended_at=$2,end_reason='INITIAL_PAYMENT_EXPIRED',version=version+1 \
         WHERE customer_plan_id=$1 AND activation_status='PENDING_INITIAL_PAYMENT'",
    )
    .bind(request.customer_plan_id)
    .bind(as_of)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn update_renewal_plan(
    transaction: &mut Transaction<'_, Postgres>,
    request: &DueCollection,
) -> Result<bool, sqlx::Error> {
    if request.request_kind != "RENEWAL" {
        return Ok(false);
    }
    let result = sqlx::query(
        "UPDATE customer_plans SET commercial_status='PAST_DUE',renewal_status='RENEWAL_INACTIVE', \
         version=version+1 WHERE customer_plan_id=$1 AND commercial_status='ACTIVE_PAID' \
         AND renewal_status='CURRENT'",
    )
    .bind(request.customer_plan_id)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn insert_expiration_event(
    transaction: &mut Transaction<'_, Postgres>,
    request: &DueCollection,
    as_of: DateTime<Utc>,
) -> ApiResult<()> {
    let event_id = Uuid::new_v4();
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(aggregate_sequence),0)+1 FROM outbox_events \
         WHERE aggregate_type='collection_request' AND aggregate_id=$1",
    )
    .bind(request.request_id)
    .fetch_one(&mut **transaction)
    .await?;
    let payload = json!({"billing_event_id":event_id,"event_type":"collection.expired",
        "schema_version":1,"occurred_at":as_of,"workspace_id":request.workspace_id,
        "correlation_id":request.correlation_id,"collection_request_id":request.request_id,
        "customer_plan_id":request.customer_plan_id,"request_kind":request.request_kind,
        "terminal_reason":"PAYMENT_WINDOW_EXPIRED"});
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         workspace_id,correlation_id,payload) VALUES \
         ($1,'collection.expired','collection_request',$2,$3,$4,$5,$6)",
    )
    .bind(event_id)
    .bind(request.request_id)
    .bind(sequence)
    .bind(request.workspace_id)
    .bind(request.correlation_id)
    .bind(payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
