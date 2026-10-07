use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    dto::{
        catalog::{CatalogStatus, UsageModel},
        plans::{
            AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest, CreateOnDemandPlanRequest,
            CreatePlanTransitionRequest, CreateSubscriptionPlanRequest, CreateSubscriptionRequest,
            CustomerPlanResponse, OnDemandPlanResponse, PlanTransitionKind, PlanTransitionResponse,
            RevokeCustomerPlanRequest, RevokePlanRequest, RunSubscriptionCyclesResponse,
            SubscriptionModel, SubscriptionPlanResponse, SubscriptionResponse,
        },
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
    services::calendar::cycle_end,
};

pub async fn create_subscription(
    repository: &DatabaseRepository,
    request: CreateSubscriptionRequest,
) -> ApiResult<SubscriptionResponse> {
    validate_name("subscription name", &request.name)?;
    if request.subscription_model != SubscriptionModel::CreditStrict {
        return Err(ApiError::unprocessable(
            "subscription_model_not_publishable",
            format!(
                "subscription model {} is unavailable in V1",
                request.subscription_model.as_str()
            ),
        ));
    }
    repository.insert_subscription(&request).await
}

pub async fn get_subscription(
    repository: &DatabaseRepository,
    subscription_id: Uuid,
) -> ApiResult<SubscriptionResponse> {
    repository.find_subscription(subscription_id).await
}

pub async fn create_plan(
    repository: &DatabaseRepository,
    subscription_id: Uuid,
    request: CreateSubscriptionPlanRequest,
) -> ApiResult<SubscriptionPlanResponse> {
    validate_plan(repository, subscription_id, &request).await?;
    repository
        .insert_subscription_plan(subscription_id, &request)
        .await
}

pub async fn get_plan(
    repository: &DatabaseRepository,
    plan_id: Uuid,
) -> ApiResult<SubscriptionPlanResponse> {
    Ok(repository.find_plan_record(plan_id).await?.response)
}

pub async fn revoke_plan(
    repository: &DatabaseRepository,
    plan_id: Uuid,
    request: RevokePlanRequest,
) -> ApiResult<SubscriptionPlanResponse> {
    validate_text("revocation reason", &request.reason, 500)?;
    validate_identifier("actor_reference", &request.actor_reference)?;
    repository
        .revoke_subscription_plan(plan_id, &request.reason, &request.actor_reference)
        .await
}

pub async fn create_on_demand_plan(
    repository: &DatabaseRepository,
    subscription_id: Uuid,
    request: CreateOnDemandPlanRequest,
) -> ApiResult<OnDemandPlanResponse> {
    repository.find_subscription(subscription_id).await?;
    validate_name("on-demand plan name", &request.name)?;
    validate_paid_terms(
        request.price_amount_minor,
        &request.currency,
        &["CARD".to_string()],
    )?;
    if request.credit_units.value() <= 0 {
        return Err(ApiError::unprocessable(
            "invalid_on_demand_credit",
            format!(
                "on-demand credit_units {} must be positive",
                request.credit_units.value()
            ),
        ));
    }
    repository
        .insert_on_demand_plan(subscription_id, &request)
        .await
}

/// Read an on-demand offer; e.g. `get_on_demand_plan(&repo, id).await`.
pub async fn get_on_demand_plan(
    repository: &DatabaseRepository,
    id: Uuid,
) -> ApiResult<OnDemandPlanResponse> {
    repository.find_on_demand_plan(id).await
}

pub async fn create_customer_plan(
    repository: &DatabaseRepository,
    account_id: Uuid,
    idempotency_key: &str,
    request: CreateCustomerPlanRequest,
) -> ApiResult<CustomerPlanResponse> {
    validate_identifier("Idempotency-Key", idempotency_key)?;
    validate_identifier("transaction_id", &request.transaction_id)?;
    let plan = repository.find_plan_record(request.plan_version_id).await?;
    if plan.response.revoked_at.is_some() {
        return Err(ApiError::conflict(
            "subscription_plan_revoked",
            format!("subscription plan {} is revoked", request.plan_version_id),
        ));
    }
    ensure_supported_admission(&plan.response)?;
    let anchor = repository.current_time().await?;
    let end = cycle_end(anchor, plan.response.recurrence, 1)?;
    let hash = request_hash(&request)?;
    repository
        .create_customer_plan(
            account_id,
            idempotency_key,
            &hash,
            &request,
            &plan,
            anchor,
            end,
        )
        .await
}

pub async fn get_customer_plan(
    repository: &DatabaseRepository,
    account_id: Uuid,
    customer_plan_id: Uuid,
) -> ApiResult<CustomerPlanResponse> {
    repository
        .find_customer_plan(account_id, customer_plan_id)
        .await
}

pub async fn cancel_customer_plan(
    repository: &DatabaseRepository,
    account_id: Uuid,
    customer_plan_id: Uuid,
) -> ApiResult<CustomerPlanResponse> {
    repository
        .cancel_customer_plan(account_id, customer_plan_id)
        .await
}

pub async fn revoke_customer_plan(
    repository: &DatabaseRepository,
    account_id: Uuid,
    customer_plan_id: Uuid,
    request: RevokeCustomerPlanRequest,
) -> ApiResult<CustomerPlanResponse> {
    validate_text("revocation reason", &request.reason, 500)?;
    validate_identifier("actor_reference", &request.actor_reference)?;
    repository
        .revoke_customer_plan(
            account_id,
            customer_plan_id,
            &request.reason,
            &request.actor_reference,
        )
        .await
}

pub async fn transition_customer_plan(
    repository: &DatabaseRepository,
    account_id: Uuid,
    customer_plan_id: Uuid,
    idempotency_key: &str,
    request: CreatePlanTransitionRequest,
) -> ApiResult<PlanTransitionResponse> {
    validate_identifier("Idempotency-Key", idempotency_key)?;
    validate_identifier("transaction_id", &request.transaction_id)?;
    validate_identifier("actor_reference", &request.actor_reference)?;
    if request.transition_kind == PlanTransitionKind::Upgrade {
        return Err(ApiError::service_unavailable(
            "billing_connector_required",
            "UPGRADE requires the Billing confirmation flow before it can be applied",
        ));
    }
    let target = repository
        .find_plan_record(request.new_plan_version_id)
        .await?;
    if target.response.commercial_model != CommercialModel::Free {
        return Err(ApiError::service_unavailable(
            "billing_connector_required",
            format!(
                "target plan {} requires Billing support",
                request.new_plan_version_id
            ),
        ));
    }
    ensure_supported_admission(&target.response)?;
    if !target.response.accepted_payment_methods.is_empty() {
        return Err(ApiError::conflict(
            "plan_transition_card_validation_required",
            format!(
                "target plan {} requires validated CARD evidence before transition; expected a free target without a card requirement",
                request.new_plan_version_id
            ),
        ));
    }
    let effective_at = repository.current_time().await?;
    let period_end = cycle_end(effective_at, target.response.recurrence, 1)?;
    let hash = request_hash(&request)?;
    repository
        .apply_plan_downgrade(
            account_id,
            customer_plan_id,
            idempotency_key,
            &hash,
            &request,
            &target,
            effective_at,
            period_end,
        )
        .await
}

pub async fn run_due_cycles(
    repository: &DatabaseRepository,
    as_of: chrono::DateTime<Utc>,
) -> ApiResult<RunSubscriptionCyclesResponse> {
    let mut response = RunSubscriptionCyclesResponse {
        processed_customer_plans: 0,
        created_cycles: 0,
        canceled_customer_plans: 0,
    };
    loop {
        let due_cycles = repository.find_due_cycles(as_of).await?;
        if due_cycles.is_empty() {
            return Ok(response);
        }
        for due in due_cycles {
            let next_end = cycle_end(due.anchor_at, due.recurrence, due.cycle_ordinal + 1)?
                .ok_or_else(|| ApiError::unexpected("recurring plan must have a next boundary"))?;
            let outcome = repository
                .advance_customer_plan_cycle(due.customer_plan_id, due.cycle_id, as_of, next_end)
                .await?;
            response.processed_customer_plans += 1;
            response.created_cycles += i64::from(outcome.created_cycle);
            response.canceled_customer_plans += i64::from(outcome.canceled_plan);
        }
        if response.processed_customer_plans >= 1000 {
            return Err(ApiError::service_unavailable(
                "cycle_backlog_limit",
                format!("cycle runner reached 1000 advances at {as_of}"),
            ));
        }
    }
}

async fn validate_plan(
    repository: &DatabaseRepository,
    subscription_id: Uuid,
    request: &CreateSubscriptionPlanRequest,
) -> ApiResult<()> {
    let subscription = repository.find_subscription(subscription_id).await?;
    if subscription.subscription_model != SubscriptionModel::CreditStrict {
        return Err(ApiError::unprocessable(
            "invalid_subscription_plan",
            format!("subscription {subscription_id} must use CREDIT_STRICT in V1"),
        ));
    }
    validate_name("plan name", &request.name)?;
    if let Some(policy_id) = request.admission_policy_version_id {
        if request.admission_policy != AdmissionPolicy::ApprovalRequired {
            return Err(ApiError::unprocessable(
                "invalid_admission_policy_reference",
                format!("policy version {policy_id} requires APPROVAL_REQUIRED admission"),
            ));
        }
        repository.find_admission_policy(policy_id).await?;
    }
    if request.granted_credit_units.value() < 0 {
        return Err(ApiError::unprocessable(
            "invalid_plan_credit",
            format!(
                "granted_credit_units {} must be non-negative",
                request.granted_credit_units.value()
            ),
        ));
    }
    validate_commercial_terms(request)?;
    if request.product_ids.is_empty() {
        return Err(ApiError::unprocessable(
            "invalid_plan_products",
            "CREDIT_STRICT plan must contain at least one product",
        ));
    }
    for product_id in &request.product_ids {
        let product = repository.find_product(*product_id).await?;
        if product.usage_model != UsageModel::CreditMetered
            || product.status != CatalogStatus::Active
        {
            return Err(ApiError::unprocessable(
                "invalid_plan_product",
                format!("product {product_id} must be active and CREDIT_METERED"),
            ));
        }
    }
    Ok(())
}

fn validate_commercial_terms(request: &CreateSubscriptionPlanRequest) -> ApiResult<()> {
    match request.commercial_model {
        CommercialModel::Free
            if request.price_amount_minor.is_none()
                && request.currency.is_none()
                && request
                    .accepted_payment_methods
                    .iter()
                    .all(|method| method == "CARD") =>
        {
            Ok(())
        }
        CommercialModel::Paid => validate_paid_terms(
            request.price_amount_minor.unwrap_or_default(),
            request.currency.as_deref().unwrap_or_default(),
            &request.accepted_payment_methods,
        ),
        _ => Err(ApiError::unprocessable(
            "invalid_plan_price",
            "FREE plans forbid amount/currency; payment methods must be CARD when present",
        )),
    }
}

fn validate_paid_terms(amount: i64, currency: &str, methods: &[String]) -> ApiResult<()> {
    if amount > 0
        && currency.len() == 3
        && currency
            .chars()
            .all(|character| character.is_ascii_uppercase())
        && methods == ["CARD"]
    {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_plan_price",
        format!(
            "paid amount {amount}, currency {currency:?}, and methods {methods:?} must be positive, ISO uppercase, and CARD-only"
        ),
    ))
}

fn validate_name(label: &str, value: &str) -> ApiResult<()> {
    validate_text(label, value, 200)
}

fn validate_text(label: &str, value: &str, maximum: usize) -> ApiResult<()> {
    if (1..=maximum).contains(&value.len()) && value.trim() == value {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_commercial_text",
        format!("{label} {value:?} must contain 1 to {maximum} characters without edge whitespace"),
    ))
}

fn validate_identifier(label: &str, value: &str) -> ApiResult<()> {
    if (1..=255).contains(&value.len()) && value.trim() == value {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_identifier",
        format!("{label} {value:?} must contain 1 to 255 characters without edge whitespace"),
    ))
}

fn request_hash<T: Serialize>(request: &T) -> ApiResult<String> {
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(request).map_err(ApiError::serialization)?);
    Ok(format!("{:#x}", digest.finalize()))
}

fn ensure_supported_admission(plan: &SubscriptionPlanResponse) -> ApiResult<()> {
    if plan.admission_policy == AdmissionPolicy::Open || plan.admission_policy_version_id.is_some()
    {
        return Ok(());
    }
    Err(ApiError::conflict(
        "customer_plan_approval_required",
        format!(
            "subscription plan {} requires explicit approval; expected OPEN admission",
            plan.plan_version_id
        ),
    ))
}
