use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{billing::CollectionRequestResponse, plans::CreatePlanTransitionRequest},
    error::{ApiError, ApiResult},
    repositories::{
        billing_regularization::collection_from_row, credits::lock_active_customer_wallet,
        database::DatabaseRepository,
    },
};

impl DatabaseRepository {
    pub async fn create_paid_plan_upgrade(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
        key: &str,
        request: &CreatePlanTransitionRequest,
    ) -> ApiResult<CollectionRequestResponse> {
        let binding_id = request.payment_method_binding_id.ok_or_else(|| {
            ApiError::unprocessable(
                "payment_method_binding_required",
                format!(
                    "UPGRADE to {} requires payment_method_binding_id",
                    request.new_plan_version_id
                ),
            )
        })?;
        let mut transaction = self.pool().begin().await?;
        lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        lock_key(&mut transaction, workspace_id, key).await?;
        if let Some(existing) = existing(&mut transaction, workspace_id, key).await? {
            validate_existing(&existing, customer_plan_id, binding_id, request)?;
            transaction.commit().await?;
            return Ok(existing);
        }
        let terms = lock_terms(
            &mut transaction,
            workspace_id,
            customer_plan_id,
            binding_id,
            request,
        )
        .await?;
        super::admission::ensure_admission_evidence(
            &mut transaction,
            workspace_id,
            request.new_plan_version_id,
        )
        .await?;
        let collection = insert_upgrade(
            &mut transaction,
            workspace_id,
            customer_plan_id,
            binding_id,
            key,
            request,
            &terms,
        )
        .await?;
        sqlx::query(
            "INSERT INTO billing_plan_upgrade_contexts \
             (collection_request_id,previous_plan_version_id,actor_reference) VALUES ($1,$2,$3)",
        )
        .bind(collection.collection_request_id)
        .bind(terms.previous_plan_version_id)
        .bind(&request.actor_reference)
        .execute(&mut *transaction)
        .await?;
        insert_event(&mut transaction, workspace_id, &collection).await?;
        transaction.commit().await?;
        Ok(collection)
    }
}

struct UpgradeTerms {
    previous_plan_version_id: Uuid,
    amount_minor: i64,
    currency: String,
    credit_units: i64,
    scheduled_at: DateTime<Utc>,
    payment_expires_at: DateTime<Utc>,
}

async fn lock_key(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("billing:{workspace_id}:{key}"))
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn existing(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    key: &str,
) -> Result<Option<CollectionRequestResponse>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT * FROM collection_requests WHERE workspace_id=$1 AND idempotency_key=$2 FOR UPDATE",
    )
    .bind(workspace_id)
    .bind(key)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.map(|row| collection_from_row(&row)))
}

fn validate_existing(
    existing: &CollectionRequestResponse,
    customer_plan_id: Uuid,
    binding_id: Uuid,
    request: &CreatePlanTransitionRequest,
) -> ApiResult<()> {
    if existing.customer_plan_id == customer_plan_id
        && existing.payment_method_binding_id == binding_id
        && existing.request_kind == "PLAN_UPGRADE"
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

async fn lock_terms(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    binding_id: Uuid,
    request: &CreatePlanTransitionRequest,
) -> ApiResult<UpgradeTerms> {
    let row = sqlx::query("SELECT cp.plan_version_id previous_plan_version_id,cp.commercial_status, \
         cp.activation_status,cp.renewal_status,current_plan.subscription_id current_subscription_id, \
         target.subscription_id target_subscription_id,target.commercial_model,target.price_amount_minor, \
         target.currency,target.granted_credit_units,target.revoked_at,pmb.status binding_status, \
         statement_timestamp() scheduled_at,statement_timestamp()+s.payment_completion_window payment_expires_at \
         FROM customer_plans cp JOIN subscription_plan_versions current_plan ON current_plan.plan_version_id=cp.plan_version_id \
         JOIN subscription_plan_versions target ON target.plan_version_id=$3 JOIN subscriptions s ON s.subscription_id=target.subscription_id \
         JOIN payment_method_bindings pmb ON pmb.payment_method_binding_id=$4 AND pmb.workspace_id=$1 AND pmb.customer_id=$1 \
         WHERE cp.customer_plan_id=$2 AND cp.customer_id=$1 FOR UPDATE OF cp,current_plan,target,pmb")
        .bind(workspace_id).bind(customer_plan_id).bind(request.new_plan_version_id).bind(binding_id)
        .fetch_optional(&mut **transaction).await?.ok_or_else(|| ApiError::not_found(
            "plan_upgrade_resources_not_found", format!("upgrade resources must belong to workspace {workspace_id}"),
        ))?;
    validate_state(customer_plan_id, &row)?;
    Ok(UpgradeTerms {
        previous_plan_version_id: row.get("previous_plan_version_id"),
        amount_minor: row.get("price_amount_minor"),
        currency: row.get("currency"),
        credit_units: row.get("granted_credit_units"),
        scheduled_at: row.get("scheduled_at"),
        payment_expires_at: row.get("payment_expires_at"),
    })
}

fn validate_state(customer_plan_id: Uuid, row: &sqlx::postgres::PgRow) -> ApiResult<()> {
    let status: String = row.get("commercial_status");
    if status == "REVOKED" {
        return Err(ApiError::conflict(
            "customer_plan_revoked",
            format!("customer plan {customer_plan_id} is revoked"),
        ));
    }
    let valid = matches!(status.as_str(), "ACTIVE" | "ACTIVE_PAID")
        && row.get::<String, _>("activation_status") == "ACTIVATED"
        && row.get::<String, _>("renewal_status") == "CURRENT"
        && row.get::<Uuid, _>("current_subscription_id")
            == row.get::<Uuid, _>("target_subscription_id")
        && row.get::<String, _>("commercial_model") == "PAID"
        && row.get::<Option<DateTime<Utc>>, _>("revoked_at").is_none()
        && row.get::<String, _>("binding_status") == "ACTIVE";
    if valid {
        return Ok(());
    }
    Err(ApiError::conflict("customer_plan_not_upgradeable", format!(
        "customer plan {customer_plan_id} must be active/current and target a paid version in the same subscription"
    )))
}

#[allow(clippy::too_many_arguments)]
async fn insert_upgrade(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    binding_id: Uuid,
    key: &str,
    request: &CreatePlanTransitionRequest,
    terms: &UpgradeTerms,
) -> ApiResult<CollectionRequestResponse> {
    let row = sqlx::query("INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id, \
         customer_plan_id,plan_version_id,payment_method_binding_id,request_kind,amount_minor,currency, \
         granted_credit_units,status,transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
         VALUES ($1,$2,$2,$3,$4,$5,'PLAN_UPGRADE',$6,$7,$8,'SCHEDULED',$9,$10,$11,$12,$13) RETURNING *")
        .bind(Uuid::new_v4()).bind(workspace_id).bind(customer_plan_id).bind(request.new_plan_version_id)
        .bind(binding_id).bind(terms.amount_minor).bind(&terms.currency).bind(terms.credit_units)
        .bind(&request.transaction_id).bind(key).bind(Uuid::new_v4()).bind(terms.scheduled_at)
        .bind(terms.payment_expires_at).fetch_one(&mut **transaction).await?;
    Ok(collection_from_row(&row))
}

async fn insert_event(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    collection: &CollectionRequestResponse,
) -> ApiResult<()> {
    let event_id = Uuid::new_v4();
    let payload = json!({"billing_event_id":event_id,"event_type":"collection.plan_upgrade_created",
        "schema_version":1,"occurred_at":collection.scheduled_at,"workspace_id":workspace_id,
        "collection_request_id":collection.collection_request_id,"customer_plan_id":collection.customer_plan_id});
    sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         workspace_id,correlation_id,payload) SELECT $1,'collection.plan_upgrade_created','collection_request',$2,1,$3,correlation_id,$4 \
         FROM collection_requests WHERE collection_request_id=$2")
        .bind(event_id).bind(collection.collection_request_id).bind(workspace_id).bind(payload)
        .execute(&mut **transaction).await?;
    Ok(())
}
