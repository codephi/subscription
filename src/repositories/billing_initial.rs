use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::billing::{CollectionRequestResponse, CreateInitialCollectionRequest},
    error::{ApiError, ApiResult},
    repositories::{
        billing_regularization::collection_from_row, credits::lock_active_customer_wallet,
        database::DatabaseRepository,
    },
};

impl DatabaseRepository {
    pub async fn create_initial_collection(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
        key: &str,
        request: &CreateInitialCollectionRequest,
    ) -> ApiResult<CollectionRequestResponse> {
        self.create_initial_collection_for_checkout(
            workspace_id,
            customer_plan_id,
            key,
            request,
            None,
        )
        .await
    }

    pub async fn create_initial_collection_for_checkout(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
        key: &str,
        request: &CreateInitialCollectionRequest,
        checkout: Option<(Uuid, &str)>,
    ) -> ApiResult<CollectionRequestResponse> {
        let mut transaction = self.pool().begin().await?;
        lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("billing:{workspace_id}:{key}"))
            .execute(&mut *transaction)
            .await?;
        if let Some(row) = sqlx::query("SELECT * FROM collection_requests WHERE workspace_id=$1 AND idempotency_key=$2 FOR UPDATE")
            .bind(workspace_id).bind(key).fetch_optional(&mut *transaction).await? {
            let existing = collection_from_row(&row);
            validate_existing(&existing, customer_plan_id, request)?;
            transaction.commit().await?;
            return Ok(existing);
        }
        let terms = lock_terms(
            &mut transaction,
            workspace_id,
            customer_plan_id,
            request.payment_method_binding_id,
        )
        .await?;
        let collection_id = Uuid::new_v4();
        let coupon = if let Some((_, code)) = checkout {
            Some(
                super::billing_checkouts::lock_coupon_discount(
                    &mut transaction,
                    workspace_id,
                    crate::dto::checkouts::CheckoutKind::Initial,
                    code,
                    terms.amount_minor,
                    &terms.currency,
                )
                .await?,
            )
        } else {
            None
        };
        let amount = coupon
            .as_ref()
            .map_or(terms.amount_minor, |value| value.final_amount_minor);
        let row = sqlx::query("INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id, \
             customer_plan_id,plan_version_id,payment_method_binding_id,request_kind,amount_minor,currency, \
             granted_credit_units,status,transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at,
             coupon_id,coupon_code,base_amount_minor,discount_amount_minor,coupon_version) \
             VALUES ($1,$2,$2,$3,$4,$5,'INITIAL',$6,$7,$8,'SCHEDULED',$9,$10,$11,$12,$13,$14,$15,$16,$17,$18) RETURNING *")
            .bind(collection_id).bind(workspace_id).bind(customer_plan_id).bind(terms.plan_version_id)
            .bind(request.payment_method_binding_id).bind(amount).bind(&terms.currency)
            .bind(terms.credit_units).bind(&request.transaction_id).bind(key).bind(Uuid::new_v4())
            .bind(terms.scheduled_at).bind(terms.payment_expires_at)
            .bind(coupon.as_ref().map(|value| value.coupon_id))
            .bind(coupon.as_ref().map(|value| value.code.as_str()))
            .bind(coupon.as_ref().map(|value| value.base_amount_minor))
            .bind(coupon.as_ref().map_or(0, |value| value.discount_amount_minor))
            .bind(coupon.as_ref().map(|value| value.version))
            .fetch_one(&mut *transaction).await?;
        let collection = collection_from_row(&row);
        if let (Some((checkout_id, _)), Some(coupon)) = (checkout, coupon.as_ref()) {
            super::billing_checkouts::store_coupon_reservation(
                &mut transaction,
                workspace_id,
                checkout_id,
                collection_id,
                crate::dto::checkouts::CheckoutKind::Initial,
                coupon,
            )
            .await?;
        }
        insert_event(&mut transaction, workspace_id, &collection).await?;
        transaction.commit().await?;
        Ok(collection)
    }
}

struct InitialTerms {
    plan_version_id: Uuid,
    amount_minor: i64,
    currency: String,
    credit_units: i64,
    scheduled_at: DateTime<Utc>,
    payment_expires_at: DateTime<Utc>,
}

async fn lock_terms(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    binding_id: Uuid,
) -> ApiResult<InitialTerms> {
    let row = sqlx::query("SELECT cp.plan_version_id,cp.commercial_status,cp.activation_status, \
         p.commercial_model,p.price_amount_minor,p.currency,p.granted_credit_units,p.revoked_at, \
         pmb.status binding_status,statement_timestamp() scheduled_at, \
         statement_timestamp()+s.payment_completion_window payment_expires_at \
         FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) \
         JOIN subscriptions s USING(subscription_id) JOIN payment_method_bindings pmb \
           ON pmb.payment_method_binding_id=$3 AND pmb.workspace_id=$1 AND pmb.customer_id=$1 \
         WHERE cp.customer_plan_id=$2 AND cp.customer_id=$1 FOR UPDATE OF cp,p,pmb")
        .bind(workspace_id).bind(customer_plan_id).bind(binding_id).fetch_optional(&mut **transaction).await?
        .ok_or_else(|| ApiError::not_found("initial_collection_resources_not_found", format!(
            "customer plan {customer_plan_id} and binding {binding_id} must belong to workspace {workspace_id}"
        )))?;
    validate_terms(customer_plan_id, &row)?;
    Ok(InitialTerms {
        plan_version_id: row.get("plan_version_id"),
        amount_minor: row.get("price_amount_minor"),
        currency: row.get("currency"),
        credit_units: row.get("granted_credit_units"),
        scheduled_at: row.get("scheduled_at"),
        payment_expires_at: row.get("payment_expires_at"),
    })
}

fn validate_terms(customer_plan_id: Uuid, row: &sqlx::postgres::PgRow) -> ApiResult<()> {
    let valid = row.get::<String, _>("commercial_status") == "ACTIVE"
        && row.get::<String, _>("activation_status") == "PENDING_INITIAL_PAYMENT"
        && row.get::<String, _>("commercial_model") == "PAID"
        && row.get::<Option<DateTime<Utc>>, _>("revoked_at").is_none()
        && row.get::<String, _>("binding_status") == "ACTIVE";
    if valid {
        return Ok(());
    }
    Err(ApiError::conflict(
        "customer_plan_not_collectable",
        format!(
            "customer plan {customer_plan_id} must await initial paid activation with active card"
        ),
    ))
}

fn validate_existing(
    existing: &CollectionRequestResponse,
    customer_plan_id: Uuid,
    request: &CreateInitialCollectionRequest,
) -> ApiResult<()> {
    if existing.customer_plan_id == customer_plan_id
        && existing.payment_method_binding_id == request.payment_method_binding_id
        && existing.request_kind == "INITIAL"
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

async fn insert_event(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    collection: &CollectionRequestResponse,
) -> ApiResult<()> {
    let event_id = Uuid::new_v4();
    let payload = json!({"billing_event_id":event_id,"event_type":"collection.initial_created",
        "schema_version":1,"occurred_at":collection.scheduled_at,"workspace_id":workspace_id,
        "collection_request_id":collection.collection_request_id,"customer_plan_id":collection.customer_plan_id});
    sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         workspace_id,correlation_id,payload) SELECT $1,'collection.initial_created','collection_request',$2,1,$3,correlation_id,$4 \
         FROM collection_requests WHERE collection_request_id=$2")
        .bind(event_id).bind(collection.collection_request_id).bind(workspace_id).bind(payload)
        .execute(&mut **transaction).await?;
    Ok(())
}
