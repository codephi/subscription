use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    repositories::{
        billing_connector::{
            BillingPaymentMethod, CollectionCommand, ConnectorCollectionResult,
            ConnectorCollectionState,
        },
        database::DatabaseRepository,
    },
};

pub struct StartedCollectionAttempt {
    pub attempt_id: Uuid,
    pub request_id: Uuid,
    pub command: CollectionCommand,
}

pub struct DueCollectionAttempt {
    pub collection_request_id: Uuid,
    pub workspace_id: Uuid,
    pub billing_connection_id: Uuid,
    pub provider: String,
    pub external_account_reference: String,
    pub secret_reference: String,
}

impl DatabaseRepository {
    pub async fn find_due_collection_attempt(&self) -> ApiResult<Option<DueCollectionAttempt>> {
        let row = sqlx::query(
            "SELECT cr.collection_request_id,bc.workspace_id,bc.billing_connection_id,bc.provider,bc.external_account_reference,bc.secret_reference \
             FROM collection_requests cr JOIN payment_method_bindings pmb USING(payment_method_binding_id) \
             JOIN billing_connections bc USING(billing_connection_id) WHERE cr.status='SCHEDULED' \
             AND cr.attempts_started=0 AND cr.scheduled_at<=clock_timestamp() \
             AND cr.payment_expires_at>clock_timestamp() AND bc.status='ACTIVE' \
             AND bc.provider='STRIPE' \
             ORDER BY cr.scheduled_at,cr.collection_request_id LIMIT 1",
        )
        .fetch_optional(&self.pool())
        .await?;
        Ok(row.map(|row| DueCollectionAttempt {
            collection_request_id: row.get("collection_request_id"),
            workspace_id: row.get("workspace_id"),
            billing_connection_id: row.get("billing_connection_id"),
            provider: row.get("provider"),
            external_account_reference: row.get("external_account_reference"),
            secret_reference: row.get("secret_reference"),
        }))
    }

    pub async fn begin_collection_attempt(
        &self,
        request_id: Uuid,
    ) -> ApiResult<Option<StartedCollectionAttempt>> {
        let mut transaction = self.pool().begin().await?;
        let row = lock_scheduled_request(&mut transaction, request_id).await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        validate_collection_dependencies(request_id, &row)?;
        let attempt = insert_started_attempt(&mut transaction, request_id, &row).await?;
        mark_collection_started(&mut transaction, request_id).await?;
        insert_attempt_event(
            &mut transaction,
            &attempt,
            "collection.attempt_requested",
            1,
        )
        .await?;
        transaction.commit().await?;
        Ok(Some(attempt))
    }

    pub async fn record_collection_result(
        &self,
        attempt: &StartedCollectionAttempt,
        result: &ConnectorCollectionResult,
    ) -> ApiResult<()> {
        let mut transaction = self.pool().begin().await?;
        let normalized = normalized_result(result);
        if !update_attempt(&mut transaction, attempt, result, &normalized).await? {
            transaction.commit().await?;
            return Ok(());
        }
        update_payment(&mut transaction, attempt, result, &normalized).await?;
        let request_updated =
            update_collection_request(&mut transaction, attempt.request_id, result, &normalized)
                .await?;
        if request_updated {
            update_plan_after_failure(&mut transaction, attempt.request_id, &normalized).await?;
        }
        insert_attempt_event(&mut transaction, attempt, normalized.event_type, 2).await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn lock_scheduled_request(
    transaction: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
) -> ApiResult<Option<sqlx::postgres::PgRow>> {
    Ok(sqlx::query(
        "SELECT cr.*,clock_timestamp() AS database_now, \
         pmb.provider_payment_method_reference,pmb.payment_method, \
         pmb.status AS binding_status,bc.provider,bc.status AS connection_status, \
         COALESCE(bc.provider_customer_reference,CASE WHEN bc.environment IS NULL \
           AND bc.external_account_reference LIKE 'cus_%' THEN bc.external_account_reference END) \
           AS provider_customer_reference \
         FROM collection_requests cr JOIN payment_method_bindings pmb \
           ON pmb.payment_method_binding_id=cr.payment_method_binding_id \
           AND pmb.workspace_id=cr.workspace_id AND pmb.customer_id=cr.customer_id \
         JOIN billing_connections bc ON bc.billing_connection_id=pmb.billing_connection_id \
           AND bc.workspace_id=cr.workspace_id \
         WHERE cr.collection_request_id=$1 AND cr.status='SCHEDULED' \
           AND cr.attempts_started=0 AND cr.scheduled_at<=clock_timestamp() FOR UPDATE OF cr",
    )
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

fn validate_collection_dependencies(
    request_id: Uuid,
    row: &sqlx::postgres::PgRow,
) -> ApiResult<()> {
    let expires_at: DateTime<Utc> = row.get("payment_expires_at");
    let database_now: DateTime<Utc> = row.get("database_now");
    if expires_at <= database_now {
        return Err(ApiError::conflict(
            "collection_request_expired",
            format!("collection request {request_id} expired at {expires_at}"),
        ));
    }
    if row.get::<String, _>("binding_status") == "ACTIVE"
        && row.get::<String, _>("connection_status") == "ACTIVE"
    {
        return Ok(());
    }
    Err(ApiError::conflict(
        "billing_binding_not_usable",
        format!("collection request {request_id} requires active connection and card binding"),
    ))
}

async fn insert_started_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
    row: &sqlx::postgres::PgRow,
) -> ApiResult<StartedCollectionAttempt> {
    let attempt_id = Uuid::new_v4();
    let connector: String = row.get("provider");
    let key = format!("collection:{request_id}:attempt:1");
    sqlx::query(
        "INSERT INTO collection_attempts (collection_attempt_id,collection_request_id, \
         attempt_number,connector,payment_method,provider_idempotency_key,status,scheduled_at,started_at) \
         VALUES ($1,$2,1,$3,'CARD',$4,'STARTED',$5,clock_timestamp())",
    )
    .bind(attempt_id)
    .bind(request_id)
    .bind(&connector)
    .bind(&key)
    .bind(row.get::<DateTime<Utc>, _>("scheduled_at"))
    .execute(&mut **transaction)
    .await?;
    insert_pending_payment(transaction, attempt_id, request_id, &connector, row).await?;
    Ok(StartedCollectionAttempt {
        attempt_id,
        request_id,
        command: command_from_row(row, key),
    })
}

fn command_from_row(row: &sqlx::postgres::PgRow, key: String) -> CollectionCommand {
    CollectionCommand {
        provider_idempotency_key: key,
        payment_method: BillingPaymentMethod::Card,
        payment_method_reference: row.get("provider_payment_method_reference"),
        customer_reference: row.try_get("provider_customer_reference").unwrap_or(None),
        amount_minor: row.get("amount_minor"),
        currency: row.get("currency"),
    }
}

async fn insert_pending_payment(
    transaction: &mut Transaction<'_, Postgres>,
    attempt_id: Uuid,
    request_id: Uuid,
    connector: &str,
    row: &sqlx::postgres::PgRow,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO billing_payments (billing_payment_id,collection_request_id, \
         collection_attempt_id,provider,state,amount_minor,currency) \
         VALUES ($1,$2,$3,$4,'PENDING',$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(request_id)
    .bind(attempt_id)
    .bind(connector)
    .bind(row.get::<i64, _>("amount_minor"))
    .bind(row.get::<String, _>("currency"))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn mark_collection_started(
    transaction: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collection_requests SET status='COLLECTING',attempts_started=1 \
         WHERE collection_request_id=$1",
    )
    .bind(request_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

struct NormalizedResult {
    attempt_status: &'static str,
    payment_state: &'static str,
    request_status: &'static str,
    event_type: &'static str,
}

fn normalized_result(result: &ConnectorCollectionResult) -> NormalizedResult {
    match result.state {
        ConnectorCollectionState::Pending => normalized(
            "PENDING",
            "PENDING",
            "PENDING_PAYMENT",
            "collection.attempt_pending",
        ),
        ConnectorCollectionState::RequiresAction => normalized(
            "REQUIRES_ACTION",
            "REQUIRES_ACTION",
            "PENDING_PAYMENT",
            "collection.awaiting_customer",
        ),
        ConnectorCollectionState::Failed => {
            normalized("FAILED", "FAILED", "EXHAUSTED", "collection.attempt_failed")
        }
        ConnectorCollectionState::Uncertain => normalized(
            "UNCERTAIN",
            "PENDING",
            "PENDING_PAYMENT",
            "collection.attempt_pending",
        ),
    }
}

fn normalized(
    attempt_status: &'static str,
    payment_state: &'static str,
    request_status: &'static str,
    event_type: &'static str,
) -> NormalizedResult {
    NormalizedResult {
        attempt_status,
        payment_state,
        request_status,
        event_type,
    }
}

async fn update_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    attempt: &StartedCollectionAttempt,
    result: &ConnectorCollectionResult,
    normalized: &NormalizedResult,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query(
        "UPDATE collection_attempts SET status=$2,finished_at=CASE WHEN $2 IN ('FAILED','SUCCEEDED') \
         THEN clock_timestamp() ELSE NULL END,failure_code=$3,next_action_url=$4 \
         WHERE collection_attempt_id=$1 AND status='STARTED'",
    )
    .bind(attempt.attempt_id)
    .bind(normalized.attempt_status)
    .bind(&result.failure_code)
    .bind(&result.next_action_url)
    .execute(&mut **transaction)
    .await?;
    Ok(updated.rows_affected() == 1)
}

async fn update_payment(
    transaction: &mut Transaction<'_, Postgres>,
    attempt: &StartedCollectionAttempt,
    result: &ConnectorCollectionResult,
    normalized: &NormalizedResult,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE billing_payments SET state=$2,provider_payment_id=COALESCE(provider_payment_id,$3), \
         failure_code=$4 WHERE collection_attempt_id=$1 AND state='PENDING'",
    )
    .bind(attempt.attempt_id)
    .bind(normalized.payment_state)
    .bind(&result.provider_payment_id)
    .bind(&result.failure_code)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn update_collection_request(
    transaction: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
    result: &ConnectorCollectionResult,
    normalized: &NormalizedResult,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query(
        "UPDATE collection_requests SET status=$2,terminal_reason=CASE WHEN $2='EXHAUSTED' \
         THEN COALESCE($3,'PROVIDER_FAILURE') ELSE terminal_reason END \
         WHERE collection_request_id=$1 AND status='COLLECTING'",
    )
    .bind(request_id)
    .bind(normalized.request_status)
    .bind(&result.failure_code)
    .execute(&mut **transaction)
    .await?;
    Ok(updated.rows_affected() == 1)
}

async fn update_plan_after_failure(
    transaction: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
    normalized: &NormalizedResult,
) -> Result<(), sqlx::Error> {
    if normalized.request_status != "EXHAUSTED" {
        return Ok(());
    }
    sqlx::query(
        "UPDATE customer_plans cp SET commercial_status=CASE cr.request_kind \
           WHEN 'INITIAL' THEN 'CANCELED' WHEN 'RENEWAL' THEN 'PAST_DUE' ELSE cp.commercial_status END, \
         activation_status=CASE WHEN cr.request_kind='INITIAL' THEN 'FAILED' ELSE cp.activation_status END, \
         renewal_status=CASE WHEN cr.request_kind IN ('INITIAL','RENEWAL') \
           THEN 'RENEWAL_INACTIVE' ELSE cp.renewal_status END, \
         ended_at=CASE WHEN cr.request_kind='INITIAL' THEN clock_timestamp() ELSE cp.ended_at END, \
         end_reason=CASE WHEN cr.request_kind='INITIAL' THEN 'INITIAL_PAYMENT_FAILED' ELSE cp.end_reason END, \
         version=cp.version+1 FROM collection_requests cr WHERE cr.collection_request_id=$1 \
         AND cr.customer_plan_id=cp.customer_plan_id AND ((cr.request_kind='INITIAL' \
           AND cp.activation_status='PENDING_INITIAL_PAYMENT') OR (cr.request_kind='RENEWAL' \
           AND cp.commercial_status='ACTIVE_PAID' AND cp.renewal_status='CURRENT'))",
    )
    .bind(request_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_attempt_event(
    transaction: &mut Transaction<'_, Postgres>,
    attempt: &StartedCollectionAttempt,
    event_type: &str,
    sequence: i64,
) -> ApiResult<()> {
    let workspace_id: Uuid = sqlx::query_scalar(
        "SELECT workspace_id FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(attempt.request_id)
    .fetch_one(&mut **transaction)
    .await?;
    let event_id = Uuid::new_v4();
    let correlation_id: Uuid = sqlx::query_scalar(
        "SELECT correlation_id FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(attempt.request_id)
    .fetch_one(&mut **transaction)
    .await?;
    let payload = json!({"billing_event_id":event_id,"event_type":event_type,"schema_version":1,
        "occurred_at":Utc::now(),"workspace_id":workspace_id,"correlation_id":correlation_id,
        "collection_request_id":attempt.request_id,"collection_attempt_id":attempt.attempt_id,"attempt_number":1});
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         workspace_id,correlation_id,payload) VALUES ($1,$2,'collection_request',$3,$4,$5,$6,$7)",
    )
    .bind(event_id).bind(event_type).bind(attempt.request_id).bind(sequence)
    .bind(workspace_id).bind(correlation_id).bind(payload)
    .execute(&mut **transaction).await?;
    Ok(())
}
