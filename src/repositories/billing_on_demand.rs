use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::billing::{CollectionRequestResponse, CreateOnDemandPurchaseRequest},
    error::{ApiError, ApiResult},
    repositories::{
        billing_regularization::collection_from_row, credits::lock_active_customer_wallet,
        database::DatabaseRepository,
    },
};

impl DatabaseRepository {
    pub async fn create_on_demand_purchase(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
        idempotency_key: &str,
        request: &CreateOnDemandPurchaseRequest,
    ) -> ApiResult<CollectionRequestResponse> {
        self.create_on_demand_purchase_for_checkout(
            workspace_id,
            customer_plan_id,
            idempotency_key,
            request,
            None,
        )
        .await
    }

    pub async fn create_on_demand_purchase_for_checkout(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
        idempotency_key: &str,
        request: &CreateOnDemandPurchaseRequest,
        checkout: Option<(Uuid, &str)>,
    ) -> ApiResult<CollectionRequestResponse> {
        let mut transaction = self.pool().begin().await?;
        lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        lock_key(&mut transaction, workspace_id, idempotency_key).await?;
        if let Some(existing) =
            existing_purchase(&mut transaction, workspace_id, idempotency_key).await?
        {
            validate_existing(&existing, customer_plan_id, request)?;
            transaction.commit().await?;
            return Ok(existing);
        }
        let terms =
            lock_purchase_terms(&mut transaction, workspace_id, customer_plan_id, request).await?;
        let collection_id = Uuid::new_v4();
        let coupon = if let Some((_, code)) = checkout {
            Some(
                super::billing_checkouts::lock_coupon_discount(
                    &mut transaction,
                    workspace_id,
                    crate::dto::checkouts::CheckoutKind::OnDemand,
                    code,
                    terms.amount_minor,
                    &terms.currency,
                )
                .await?,
            )
        } else {
            None
        };
        let collection = insert_purchase(
            &mut transaction,
            PurchaseInsert {
                workspace_id,
                customer_plan_id,
                key: idempotency_key,
                request,
                terms: &terms,
                collection_id,
                coupon: coupon.as_ref(),
            },
        )
        .await?;
        if let (Some((checkout_id, _)), Some(coupon)) = (checkout, coupon.as_ref()) {
            super::billing_checkouts::store_coupon_reservation(
                &mut transaction,
                workspace_id,
                checkout_id,
                collection_id,
                crate::dto::checkouts::CheckoutKind::OnDemand,
                coupon,
            )
            .await?;
        }
        insert_purchase_event(&mut transaction, workspace_id, &collection).await?;
        transaction.commit().await?;
        Ok(collection)
    }
}

struct PurchaseTerms {
    plan_version_id: Uuid,
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

async fn existing_purchase(
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
    request: &CreateOnDemandPurchaseRequest,
) -> ApiResult<()> {
    if existing.customer_plan_id == customer_plan_id
        && existing.payment_method_binding_id == request.payment_method_binding_id
        && existing.request_kind == "ON_DEMAND"
        && existing.quantity == request.quantity
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

async fn lock_purchase_terms(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    request: &CreateOnDemandPurchaseRequest,
) -> ApiResult<PurchaseTerms> {
    if !(1..=10_000).contains(&request.quantity) {
        return Err(ApiError::unprocessable(
            "invalid_credit_quantity",
            format!(
                "credit quantity {} must be between 1 and 10000",
                request.quantity
            ),
        ));
    }
    let row = sqlx::query(
        "SELECT cp.plan_version_id,cp.commercial_status,cp.activation_status,cp.renewal_status, \
         sp.recurrence,sp.revoked_at plan_revoked_at,sp.subscription_id, \
         od.price_amount_minor,od.currency,od.credit_units,od.revoked_at on_demand_revoked_at, \
         pmb.status binding_status,statement_timestamp() scheduled_at, \
         statement_timestamp()+s.payment_completion_window payment_expires_at \
         FROM customer_plans cp JOIN subscription_plan_versions sp USING(plan_version_id) \
         JOIN subscriptions s USING(subscription_id) JOIN on_demand_plans od USING(subscription_id) \
         JOIN payment_method_bindings pmb ON pmb.payment_method_binding_id=$4 \
           AND pmb.workspace_id=$1 AND pmb.customer_id=$1 \
         WHERE cp.customer_plan_id=$2 AND cp.customer_id=$1 AND od.on_demand_plan_id=$3 \
         FOR UPDATE OF cp,sp,od,pmb",
    )
    .bind(workspace_id).bind(customer_plan_id).bind(request.on_demand_plan_id)
    .bind(request.payment_method_binding_id).fetch_optional(&mut **transaction).await?
    .ok_or_else(|| ApiError::not_found("on_demand_purchase_not_found", format!(
        "customer plan {customer_plan_id}, on-demand plan {}, and binding {} must share workspace {workspace_id}",
        request.on_demand_plan_id, request.payment_method_binding_id
    )))?;
    validate_purchase_state(customer_plan_id, &row)?;
    let unit_amount: i64 = row.get("price_amount_minor");
    let unit_credits: i64 = row.get("credit_units");
    let amount_minor = unit_amount.checked_mul(request.quantity).ok_or_else(|| {
        ApiError::unprocessable(
            "credit_quantity_overflow",
            format!(
                "unit amount {unit_amount} multiplied by quantity {} overflows",
                request.quantity
            ),
        )
    })?;
    let credit_units = unit_credits.checked_mul(request.quantity).ok_or_else(|| {
        ApiError::unprocessable(
            "credit_quantity_overflow",
            format!(
                "unit credits {unit_credits} multiplied by quantity {} overflows",
                request.quantity
            ),
        )
    })?;
    Ok(PurchaseTerms {
        plan_version_id: row.get("plan_version_id"),
        amount_minor,
        currency: row.get("currency"),
        credit_units,
        scheduled_at: row.get("scheduled_at"),
        payment_expires_at: row.get("payment_expires_at"),
    })
}

fn validate_purchase_state(customer_plan_id: Uuid, row: &sqlx::postgres::PgRow) -> ApiResult<()> {
    let commercial: String = row.get("commercial_status");
    if commercial == "REVOKED" {
        return Err(ApiError::conflict(
            "customer_plan_revoked",
            format!("customer plan {customer_plan_id} is revoked"),
        ));
    }
    if row.get::<String, _>("activation_status") != "ACTIVATED" {
        return Err(ApiError::conflict(
            "customer_plan_activation_pending",
            format!("customer plan {customer_plan_id} must be activated"),
        ));
    }
    let valid = matches!(commercial.as_str(), "ACTIVE" | "ACTIVE_PAID")
        && row
            .get::<Option<DateTime<Utc>>, _>("plan_revoked_at")
            .is_none()
        && row
            .get::<Option<DateTime<Utc>>, _>("on_demand_revoked_at")
            .is_none()
        && row.get::<String, _>("binding_status") == "ACTIVE";
    if valid {
        return Ok(());
    }
    Err(ApiError::conflict(
        "customer_plan_not_eligible_for_on_demand",
        format!(
            "customer plan {customer_plan_id} must be active with current catalog and card binding"
        ),
    ))
}

struct PurchaseInsert<'a> {
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    key: &'a str,
    request: &'a CreateOnDemandPurchaseRequest,
    terms: &'a PurchaseTerms,
    collection_id: Uuid,
    coupon: Option<&'a super::billing_checkouts::CouponDiscount>,
}

async fn insert_purchase(
    transaction: &mut Transaction<'_, Postgres>,
    input: PurchaseInsert<'_>,
) -> ApiResult<CollectionRequestResponse> {
    let PurchaseInsert {
        workspace_id,
        customer_plan_id,
        key,
        request,
        terms,
        collection_id,
        coupon,
    } = input;
    let row = sqlx::query(
        "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id,customer_plan_id, \
         plan_version_id,on_demand_plan_id,payment_method_binding_id,request_kind,amount_minor,currency, \
         granted_credit_units,credit_quantity,status,transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at,
         coupon_id,coupon_code,base_amount_minor,discount_amount_minor,coupon_version) \
         VALUES ($1,$2,$2,$3,$4,$5,$6,'ON_DEMAND',$7,$8,$9,$10,'SCHEDULED',$11,$12,$13,$14,$15,$16,$17,$18,$19,$20) RETURNING *",
    ).bind(collection_id).bind(workspace_id).bind(customer_plan_id).bind(terms.plan_version_id)
      .bind(request.on_demand_plan_id).bind(request.payment_method_binding_id)
      .bind(coupon.map_or(terms.amount_minor, |value| value.final_amount_minor))
      .bind(&terms.currency).bind(terms.credit_units).bind(request.quantity).bind(&request.transaction_id).bind(key)
      .bind(Uuid::new_v4()).bind(terms.scheduled_at).bind(terms.payment_expires_at)
      .bind(coupon.map(|value| value.coupon_id)).bind(coupon.map(|value| value.code.as_str()))
      .bind(coupon.map(|value| value.base_amount_minor))
      .bind(coupon.map_or(0, |value| value.discount_amount_minor)).bind(coupon.map(|value| value.version))
      .fetch_one(&mut **transaction).await?;
    Ok(collection_from_row(&row))
}

async fn insert_purchase_event(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    collection: &CollectionRequestResponse,
) -> ApiResult<()> {
    let event_id = Uuid::new_v4();
    let payload = json!({"billing_event_id":event_id,"event_type":"collection.on_demand_created",
        "schema_version":1,"occurred_at":collection.scheduled_at,"workspace_id":workspace_id,
        "collection_request_id":collection.collection_request_id,"customer_plan_id":collection.customer_plan_id});
    sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         workspace_id,correlation_id,payload) SELECT $1,'collection.on_demand_created','collection_request', \
         $2,1,$3,correlation_id,$4 FROM collection_requests WHERE collection_request_id=$2")
        .bind(event_id).bind(collection.collection_request_id).bind(workspace_id).bind(payload)
        .execute(&mut **transaction).await?;
    Ok(())
}
