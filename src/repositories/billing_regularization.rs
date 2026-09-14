use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::billing::{CollectionRequestResponse, CreateRenewalRegularizationRequest},
    error::{ApiError, ApiResult},
    repositories::{credits::lock_active_customer_wallet, database::DatabaseRepository},
};

impl DatabaseRepository {
    pub async fn find_collection_request(
        &self,
        workspace_id: Uuid,
        collection_request_id: Uuid,
    ) -> ApiResult<CollectionRequestResponse> {
        let row = sqlx::query(
            "SELECT * FROM collection_requests WHERE workspace_id=$1 AND collection_request_id=$2",
        )
        .bind(workspace_id)
        .bind(collection_request_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| ApiError::not_found(
            "collection_request_not_found",
            format!("collection request {collection_request_id} does not exist in workspace {workspace_id}"),
        ))?;
        Ok(collection_from_row(&row))
    }

    pub async fn create_renewal_regularization(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
        idempotency_key: &str,
        request: &CreateRenewalRegularizationRequest,
    ) -> ApiResult<CollectionRequestResponse> {
        let mut transaction = self.pool().begin().await?;
        let _wallet = lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        lock_idempotency_key(&mut transaction, workspace_id, idempotency_key).await?;
        if let Some(existing) =
            find_existing_request(&mut transaction, workspace_id, idempotency_key).await?
        {
            validate_existing_request(&existing, customer_plan_id, request)?;
            transaction.commit().await?;
            return Ok(existing);
        }
        let terms = lock_regularization_terms(
            &mut transaction,
            workspace_id,
            customer_plan_id,
            request.payment_method_binding_id,
        )
        .await?;
        let collection = insert_regularization(
            &mut transaction,
            workspace_id,
            customer_plan_id,
            idempotency_key,
            request,
            &terms,
        )
        .await?;
        insert_regularization_event(&mut transaction, workspace_id, &collection).await?;
        transaction.commit().await?;
        Ok(collection)
    }
}

struct RegularizationTerms {
    plan_version_id: Uuid,
    amount_minor: i64,
    currency: String,
    granted_credit_units: i64,
    scheduled_at: DateTime<Utc>,
    payment_expires_at: DateTime<Utc>,
}

async fn lock_idempotency_key(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    idempotency_key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("billing:{workspace_id}:{idempotency_key}"))
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn find_existing_request(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    idempotency_key: &str,
) -> Result<Option<CollectionRequestResponse>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT * FROM collection_requests WHERE workspace_id=$1 AND idempotency_key=$2 FOR UPDATE",
    )
    .bind(workspace_id)
    .bind(idempotency_key)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.map(|row| collection_from_row(&row)))
}

fn validate_existing_request(
    existing: &CollectionRequestResponse,
    customer_plan_id: Uuid,
    request: &CreateRenewalRegularizationRequest,
) -> ApiResult<()> {
    if existing.customer_plan_id == customer_plan_id
        && existing.payment_method_binding_id == request.payment_method_binding_id
        && existing.request_kind == "RENEWAL_REGULARIZATION"
        && existing.transaction_id == request.transaction_id
    {
        return Ok(());
    }
    Err(ApiError::conflict(
        "idempotency_key_already_used",
        format!(
            "idempotency key {:?} belongs to collection request {} with different parameters",
            existing.idempotency_key, existing.collection_request_id
        ),
    ))
}

async fn lock_regularization_terms(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    binding_id: Uuid,
) -> ApiResult<RegularizationTerms> {
    let row = sqlx::query(
        "SELECT cp.plan_version_id,cp.commercial_status,cp.activation_status,cp.renewal_status, \
         p.price_amount_minor,p.currency,p.granted_credit_units,p.revoked_at, \
         statement_timestamp() scheduled_at,statement_timestamp()+s.payment_completion_window payment_expires_at, \
         pmb.status binding_status \
         FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) \
         JOIN subscriptions s USING(subscription_id) JOIN payment_method_bindings pmb \
           ON pmb.payment_method_binding_id=$3 AND pmb.workspace_id=$1 AND pmb.customer_id=$1 \
         WHERE cp.customer_plan_id=$2 AND cp.customer_id=$1 FOR UPDATE OF cp,pmb",
    )
    .bind(workspace_id)
    .bind(customer_plan_id)
    .bind(binding_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| ApiError::not_found("billing_binding_not_found", format!(
        "customer plan {customer_plan_id} and payment binding {binding_id} must belong to workspace {workspace_id}"
    )))?;
    validate_regularization_state(customer_plan_id, &row)?;
    Ok(RegularizationTerms {
        plan_version_id: row.get("plan_version_id"),
        amount_minor: row.get("price_amount_minor"),
        currency: row.get("currency"),
        granted_credit_units: row.get("granted_credit_units"),
        scheduled_at: row.get("scheduled_at"),
        payment_expires_at: row.get("payment_expires_at"),
    })
}

fn validate_regularization_state(
    customer_plan_id: Uuid,
    row: &sqlx::postgres::PgRow,
) -> ApiResult<()> {
    if row.get::<Option<DateTime<Utc>>, _>("revoked_at").is_some() {
        return Err(ApiError::conflict(
            "subscription_plan_revoked",
            format!("customer plan {customer_plan_id} references a revoked plan version"),
        ));
    }
    let valid = row.get::<String, _>("commercial_status") == "PAST_DUE"
        && row.get::<String, _>("activation_status") == "ACTIVATED"
        && row.get::<String, _>("renewal_status") == "RENEWAL_INACTIVE"
        && row.get::<String, _>("binding_status") == "ACTIVE";
    if valid {
        return Ok(());
    }
    Err(ApiError::conflict(
        "customer_plan_not_regularizable",
        format!(
            "customer plan {customer_plan_id} must be activated, past due, and renewal inactive"
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
async fn insert_regularization(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    idempotency_key: &str,
    request: &CreateRenewalRegularizationRequest,
    terms: &RegularizationTerms,
) -> ApiResult<CollectionRequestResponse> {
    let row = sqlx::query(
        "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id,customer_plan_id, \
         plan_version_id,payment_method_binding_id,request_kind,amount_minor,currency,granted_credit_units, \
         status,transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
         VALUES ($1,$2,$2,$3,$4,$5,'RENEWAL_REGULARIZATION',$6,$7,$8,'SCHEDULED',$9,$10,$11,$12,$13) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(customer_plan_id)
    .bind(terms.plan_version_id)
    .bind(request.payment_method_binding_id)
    .bind(terms.amount_minor)
    .bind(&terms.currency)
    .bind(terms.granted_credit_units)
    .bind(&request.transaction_id)
    .bind(idempotency_key)
    .bind(Uuid::new_v4())
    .bind(terms.scheduled_at)
    .bind(terms.payment_expires_at)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(collection_from_row(&row))
}

async fn insert_regularization_event(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    collection: &CollectionRequestResponse,
) -> ApiResult<()> {
    let event_id = Uuid::new_v4();
    let payload = json!({"billing_event_id":event_id,"event_type":"collection.regularization_created",
        "schema_version":1,"occurred_at":collection.scheduled_at,"workspace_id":workspace_id,
        "collection_request_id":collection.collection_request_id,
        "customer_plan_id":collection.customer_plan_id});
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         workspace_id,correlation_id,payload) SELECT $1,'collection.regularization_created', \
         'collection_request',$2,1,$3,correlation_id,$4 FROM collection_requests \
         WHERE collection_request_id=$2",
    )
    .bind(event_id)
    .bind(collection.collection_request_id)
    .bind(workspace_id)
    .bind(payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) fn collection_from_row(row: &sqlx::postgres::PgRow) -> CollectionRequestResponse {
    CollectionRequestResponse {
        collection_request_id: row.get("collection_request_id"),
        customer_plan_id: row.get("customer_plan_id"),
        payment_method_binding_id: row.get("payment_method_binding_id"),
        request_kind: row.get("request_kind"),
        amount_minor: row.get("amount_minor"),
        currency: row.get("currency"),
        granted_credit_units: row.get("granted_credit_units"),
        status: row.get("status"),
        transaction_id: row.get("transaction_id"),
        idempotency_key: row.get("idempotency_key"),
        scheduled_at: row.get("scheduled_at"),
        payment_expires_at: row.get("payment_expires_at"),
    }
}
