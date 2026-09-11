use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    dto::plans::{
        CommercialModel, CreateCustomerPlanRequest, CreateOnDemandPlanRequest,
        CreateSubscriptionPlanRequest, CreateSubscriptionRequest, CustomerPlanResponse,
        OnDemandPlanResponse, SubscriptionPlanResponse, SubscriptionResponse,
    },
    error::{ApiError, ApiResult},
    repositories::{
        credit_writes::complete_reservations,
        credits::{lock_active_customer_wallet, reserve_idempotency, reserve_transaction},
        database::DatabaseRepository,
        plan_rows::{
            customer_plan_from_row, cycle_from_row, on_demand_from_row, plan_from_row,
            subscription_from_row, PlanRecord,
        },
        plan_writes::{
            activate_free_plan, ensure_recurring_credit_enabled, insert_customer_plan_row,
            lock_valid_plan, reserve_active_slot,
        },
    },
};

impl DatabaseRepository {
    pub async fn insert_subscription(
        &self,
        request: &CreateSubscriptionRequest,
    ) -> ApiResult<SubscriptionResponse> {
        let row = sqlx::query(
            "INSERT INTO subscriptions (subscription_id,name,subscription_model) VALUES ($1,$2,$3) RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(&request.name)
        .bind(request.subscription_model.as_str())
        .fetch_one(&self.pool())
        .await?;
        subscription_from_row(&row)
    }

    pub async fn find_subscription(
        &self,
        subscription_id: Uuid,
    ) -> ApiResult<SubscriptionResponse> {
        let row = sqlx::query("SELECT * FROM subscriptions WHERE subscription_id=$1")
            .bind(subscription_id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| missing("subscription", subscription_id))?;
        subscription_from_row(&row)
    }

    pub async fn insert_subscription_plan(
        &self,
        subscription_id: Uuid,
        request: &CreateSubscriptionPlanRequest,
    ) -> ApiResult<SubscriptionPlanResponse> {
        let mut transaction = self.pool().begin().await?;
        let plan_id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO subscription_plan_versions (plan_version_id,subscription_id,name, \
             commercial_model,price_amount_minor,currency,recurrence,admission_policy, \
             accepted_payment_methods,granted_credit_units) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             RETURNING *",
        )
        .bind(plan_id)
        .bind(subscription_id)
        .bind(&request.name)
        .bind(request.commercial_model.as_str())
        .bind(request.price_amount_minor)
        .bind(&request.currency)
        .bind(request.recurrence.as_str())
        .bind(request.admission_policy.as_str())
        .bind(&request.accepted_payment_methods)
        .bind(request.granted_credit_units.value())
        .fetch_one(&mut *transaction)
        .await?;
        for product_id in &request.product_ids {
            sqlx::query(
                "INSERT INTO subscription_plan_products (plan_version_id,product_id) VALUES ($1,$2)",
            )
            .bind(plan_id)
            .bind(product_id)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        plan_from_row(&row, request.product_ids.clone())
    }

    pub(crate) async fn find_plan_record(&self, plan_id: Uuid) -> ApiResult<PlanRecord> {
        let row = sqlx::query(
            "SELECT p.*,s.subscription_model FROM subscription_plan_versions p \
             JOIN subscriptions s ON s.subscription_id=p.subscription_id WHERE p.plan_version_id=$1",
        )
        .bind(plan_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing("subscription_plan", plan_id))?;
        let product_ids = load_plan_products(&self.pool(), plan_id).await?;
        Ok(PlanRecord {
            response: plan_from_row(&row, product_ids)?,
        })
    }

    pub async fn insert_on_demand_plan(
        &self,
        subscription_id: Uuid,
        request: &CreateOnDemandPlanRequest,
    ) -> ApiResult<OnDemandPlanResponse> {
        let row = sqlx::query(
            "INSERT INTO on_demand_plans (on_demand_plan_id,subscription_id,name,price_amount_minor, \
             currency,credit_units) VALUES ($1,$2,$3,$4,$5,$6) RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(subscription_id)
        .bind(&request.name)
        .bind(request.price_amount_minor)
        .bind(&request.currency)
        .bind(request.credit_units.value())
        .fetch_one(&self.pool())
        .await?;
        Ok(on_demand_from_row(&row))
    }

    pub async fn revoke_subscription_plan(
        &self,
        plan_id: Uuid,
        reason: &str,
        actor_reference: &str,
    ) -> ApiResult<SubscriptionPlanResponse> {
        let mut transaction = self.pool().begin().await?;
        let row = sqlx::query(
            "UPDATE subscription_plan_versions SET revoked_at=now(),revocation_reason=$2 \
             WHERE plan_version_id=$1 AND revoked_at IS NULL RETURNING *",
        )
        .bind(plan_id)
        .bind(reason)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "subscription_plan_not_revocable",
                format!("subscription plan {plan_id} must exist and not already be revoked"),
            )
        })?;
        sqlx::query(
            "INSERT INTO audit_events (audit_event_id,actor_reference,action,resource_kind, \
             resource_id,correlation_id,details) VALUES ($1,$2,'subscription_plan.revoked', \
             'subscription_plan',$3,$4,jsonb_build_object('reason',$5))",
        )
        .bind(Uuid::new_v4())
        .bind(actor_reference)
        .bind(plan_id)
        .bind(Uuid::new_v4())
        .bind(reason)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        let products = load_plan_products(&self.pool(), plan_id).await?;
        plan_from_row(&row, products)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_customer_plan(
        &self,
        workspace_id: Uuid,
        idempotency_key: &str,
        request_hash: &str,
        request: &CreateCustomerPlanRequest,
        plan: &PlanRecord,
        anchor_at: DateTime<Utc>,
        period_end: Option<DateTime<Utc>>,
    ) -> ApiResult<CustomerPlanResponse> {
        let mut transaction = self.pool().begin().await?;
        let wallet = lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        ensure_recurring_credit_enabled(&mut transaction, workspace_id, plan).await?;
        reserve_idempotency(
            &mut transaction,
            workspace_id,
            idempotency_key,
            request_hash,
            "CUSTOMER_PLAN_CREATE",
        )
        .await?;
        reserve_transaction(
            &mut transaction,
            workspace_id,
            &request.transaction_id,
            "CUSTOMER_PLAN_CREATE",
        )
        .await?;
        lock_valid_plan(&mut transaction, plan.response.plan_version_id).await?;
        let customer_plan_id = Uuid::new_v4();
        let activates_now = plan.response.commercial_model == CommercialModel::Free
            && plan.response.accepted_payment_methods.is_empty();
        let row = insert_customer_plan_row(
            &mut transaction,
            customer_plan_id,
            workspace_id,
            plan.response.plan_version_id,
            anchor_at,
            plan.response.commercial_model,
            activates_now,
        )
        .await?;
        reserve_active_slot(
            &mut transaction,
            workspace_id,
            plan.response.subscription_id,
            customer_plan_id,
        )
        .await?;
        let cycle = if activates_now {
            Some(
                activate_free_plan(
                    &mut transaction,
                    &wallet,
                    workspace_id,
                    customer_plan_id,
                    plan,
                    anchor_at,
                    period_end,
                    &request.transaction_id,
                )
                .await?,
            )
        } else {
            None
        };
        complete_reservations(
            &mut transaction,
            workspace_id,
            idempotency_key,
            &request.transaction_id,
            customer_plan_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(customer_plan_from_row(
            &row,
            cycle,
            if activates_now {
                plan.response.product_ids.clone()
            } else {
                Vec::new()
            },
        ))
    }

    pub async fn find_customer_plan(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
    ) -> ApiResult<CustomerPlanResponse> {
        let row = sqlx::query(
            "SELECT * FROM customer_plans WHERE customer_id=$1 AND customer_plan_id=$2",
        )
        .bind(workspace_id)
        .bind(customer_plan_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing("customer_plan", customer_plan_id))?;
        let cycle_row = sqlx::query(
            "SELECT * FROM customer_plan_cycles WHERE customer_plan_id=$1 AND status='ACTIVE'",
        )
        .bind(customer_plan_id)
        .fetch_optional(&self.pool())
        .await?;
        let products = sqlx::query_scalar(
            "SELECT product_id FROM customer_plan_entitlements WHERE customer_plan_id=$1 \
             AND effective_until IS NULL ORDER BY product_id",
        )
        .bind(customer_plan_id)
        .fetch_all(&self.pool())
        .await?;
        Ok(customer_plan_from_row(
            &row,
            cycle_row.as_ref().map(cycle_from_row),
            products,
        ))
    }
}

async fn load_plan_products(pool: &sqlx::PgPool, plan_id: Uuid) -> ApiResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT product_id FROM subscription_plan_products WHERE plan_version_id=$1 ORDER BY product_id",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?)
}

fn missing(kind: &str, id: Uuid) -> ApiError {
    ApiError::not_found(
        "commercial_resource_not_found",
        format!("{kind} {id} does not exist"),
    )
}
