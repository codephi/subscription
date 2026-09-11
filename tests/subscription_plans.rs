#[path = "support/plan_admission_regressions.rs"]
mod plan_admission_regressions;
#[path = "support/plan_calendar_regressions.rs"]
mod plan_calendar_regressions;
#[path = "support/plan_cancellation_regressions.rs"]
mod plan_cancellation_regressions;
#[path = "support/plan_scheduler_regressions.rs"]
mod plan_scheduler_regressions;
#[path = "support/subscription_plan_contract.rs"]
mod subscription_plan_contract;
#[path = "support/subscription_plan_requests.rs"]
mod subscription_plan_requests;
#[path = "support/subscription_plan_transition.rs"]
mod subscription_plan_transition;
mod support;

use std::sync::OnceLock;

use axum::Router;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use subscription::{
    dto::{
        catalog::{
            CatalogStatus, CreateItemRequest, CreatePriceVersionRequest, CreateProductRequest,
            PricingModel, UpdateItemRequest, UpdateProductRequest, UsageModel,
        },
        plans::{
            AdmissionPolicy, CommercialModel, CreateOnDemandPlanRequest, PlanRecurrence,
            RevokeCustomerPlanRequest,
        },
        units::{CreditUnits, ItemUnits},
    },
    repositories::database::DatabaseRepository,
    services::{catalog, plans},
};
use tokio::sync::Mutex;
use uuid::Uuid;

use subscription_plan_contract::{assert_plan_swagger, get_json};
use subscription_plan_requests::{
    apply_workspace_event, customer_plan_request, paid_plan_request, plan_request,
    revoke_plan_request, subscription_request,
};
use subscription_plan_transition::{
    assert_audit, assert_blocked_join, assert_downgrade, assert_plan_state, revoke_for_cleanup,
};
use support::setup_router_with_options;

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscription_cycle_grant_and_expiry_are_unique() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (router, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let plan = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 100).await;
    let customer_plan = join_plan(&repository, workspace_id, plan.plan_version_id, "cycle").await;
    let first_end = customer_plan
        .current_cycle
        .expect("initial cycle")
        .current_period_end
        .expect("period end");

    assert_plan_state(&pool, workspace_id, 1, 1, 1, 100).await;
    let advanced = plans::run_due_cycles(&repository, first_end)
        .await
        .expect("advance cycle");
    assert_eq!(advanced.created_cycles, 1);
    assert_plan_state(&pool, workspace_id, 1, 2, 3, 100).await;
    let repeated = plans::run_due_cycles(&repository, first_end)
        .await
        .expect("repeat cycle");
    assert_eq!(repeated.created_cycles, 0);
    assert_plan_state(&pool, workspace_id, 1, 2, 3, 100).await;

    let statement = get_json(
        &router,
        &format!("/v1/workspaces/{workspace_id}/customer-wallet/statement?limit=10"),
    )
    .await;
    assert_eq!(
        statement["items"][1]["entry_type"],
        "CREDIT_EXPIRY_FORFEITURE"
    );
    assert_eq!(statement["items"][1]["signed_credit_units"], "-100");
    assert_eq!(statement["items"][2]["entry_type"], "SUBSCRIPTION_CREDIT");
    assert_eq!(
        statement["items"][2]["references"]
            .as_array()
            .expect("references")
            .len(),
        4
    );
    let expiry_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE workspace_id=$1 AND event_type='credit.expired'",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .expect("expiry event count");
    assert_eq!(expiry_events, 1);
    revoke_for_cleanup(&repository, workspace_id, customer_plan.customer_plan_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn subscription_cycle_executes_once() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let plan = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 40).await;
    let customer_plan = join_plan(
        &repository,
        workspace_id,
        plan.plan_version_id,
        "concurrent-cycle",
    )
    .await;
    let end = customer_plan
        .current_cycle
        .expect("initial cycle")
        .current_period_end
        .expect("period end");
    let first_repository = repository.clone();
    let second_repository = repository.clone();

    let (first, second) = tokio::join!(
        plans::run_due_cycles(&first_repository, end),
        plans::run_due_cycles(&second_repository, end)
    );
    assert_eq!(
        first.expect("first runner").created_cycles + second.expect("second runner").created_cycles,
        1
    );
    assert_plan_state(&pool, workspace_id, 1, 2, 3, 40).await;
    revoke_for_cleanup(&repository, workspace_id, customer_plan.customer_plan_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn customer_plan_exclusivity_and_lifecycle_are_atomic() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let plan = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 25).await;
    let first_repository = repository.clone();
    let second_repository = repository.clone();
    let first_request = customer_plan_request(plan.plan_version_id, "exclusive-transaction-1");
    let second_request = customer_plan_request(plan.plan_version_id, "exclusive-transaction-2");

    let (first, second) = tokio::join!(
        plans::create_customer_plan(
            &first_repository,
            workspace_id,
            "exclusive-key-1",
            first_request
        ),
        plans::create_customer_plan(
            &second_repository,
            workspace_id,
            "exclusive-key-2",
            second_request
        )
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(
        first
            .err()
            .or_else(|| second.err())
            .expect("one conflict")
            .code(),
        "active_customer_plan_already_exists"
    );
    assert_plan_state(&pool, workspace_id, 1, 1, 1, 25).await;

    let customer_plan_id: Uuid =
        sqlx::query_scalar("SELECT customer_plan_id FROM customer_plans WHERE customer_id=$1")
            .bind(workspace_id)
            .fetch_one(&pool)
            .await
            .expect("customer plan");
    let canceled = plans::cancel_customer_plan(&repository, workspace_id, customer_plan_id)
        .await
        .expect("schedule cancel");
    assert!(canceled.cancel_at_period_end);
    assert_eq!(canceled.commercial_status, "ACTIVE");
    let end = canceled
        .current_cycle
        .expect("current cycle")
        .current_period_end
        .expect("period end");
    let outcome = plans::run_due_cycles(&repository, end)
        .await
        .expect("finish canceled plan");
    assert_eq!(outcome.canceled_customer_plans, 1);
    assert_plan_state(&pool, workspace_id, 0, 1, 2, 0).await;

    let source = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 60).await;
    let current = join_plan(
        &repository,
        workspace_id,
        source.plan_version_id,
        "transition",
    )
    .await;
    let target = plans::create_plan(
        &repository,
        source.subscription_id,
        plan_request(
            product_id,
            CommercialModel::Free,
            PlanRecurrence::Monthly,
            10,
        ),
    )
    .await
    .expect("downgrade target");
    assert_downgrade(
        &repository,
        &pool,
        workspace_id,
        current.customer_plan_id,
        target.plan_version_id,
    )
    .await;
    assert_plan_state(&pool, workspace_id, 1, 3, 3, 60).await;
    revoke_for_cleanup(&repository, workspace_id, current.customer_plan_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commercial_catalog_validation_and_swagger_are_enforced() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (router, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let subscription = plans::create_subscription(&repository, subscription_request())
        .await
        .expect("subscription");
    let rejected = plans::create_plan(
        &repository,
        subscription.subscription_id,
        plan_request(
            product_id,
            CommercialModel::Paid,
            PlanRecurrence::Monthly,
            10,
        ),
    )
    .await;
    assert_eq!(
        rejected.expect_err("invalid paid terms").code(),
        "invalid_plan_price"
    );
    let mut card_plan_request =
        plan_request(product_id, CommercialModel::Free, PlanRecurrence::None, 10);
    card_plan_request.accepted_payment_methods = vec!["CARD".to_string()];
    let card_plan =
        plans::create_plan(&repository, subscription.subscription_id, card_plan_request)
            .await
            .expect("free card plan");
    let pending = join_plan(
        &repository,
        workspace_id,
        card_plan.plan_version_id,
        "card-pending",
    )
    .await;
    assert_eq!(pending.activation_status, "PENDING_CARD_VALIDATION");
    let revoked_customer = plans::revoke_customer_plan(
        &repository,
        workspace_id,
        pending.customer_plan_id,
        RevokeCustomerPlanRequest {
            reason: "risk decision".to_string(),
            actor_reference: "operator:test".to_string(),
        },
    )
    .await
    .expect("revoke customer plan");
    assert_eq!(revoked_customer.commercial_status, "REVOKED");
    assert_eq!(
        revoked_customer.end_reason.as_deref(),
        Some("ADMIN_REVOKED")
    );
    assert_audit(
        &pool,
        pending.customer_plan_id,
        "customer_plan.revoked",
        "operator:test",
    )
    .await;
    assert_plan_state(&pool, workspace_id, 0, 0, 0, 0).await;

    let paid_plan = plans::create_plan(
        &repository,
        subscription.subscription_id,
        paid_plan_request(product_id),
    )
    .await
    .expect("paid plan");
    let paid_pending = join_plan(
        &repository,
        workspace_id,
        paid_plan.plan_version_id,
        "paid-pending",
    )
    .await;
    assert_eq!(paid_pending.activation_status, "PENDING_INITIAL_PAYMENT");
    assert!(paid_pending.current_cycle.is_none());
    plans::cancel_customer_plan(&repository, workspace_id, paid_pending.customer_plan_id)
        .await
        .expect("cancel pending plan");
    plans::create_on_demand_plan(
        &repository,
        subscription.subscription_id,
        CreateOnDemandPlanRequest {
            name: "Extra 500".to_string(),
            price_amount_minor: 500,
            currency: "BRL".to_string(),
            credit_units: CreditUnits::new(500),
        },
    )
    .await
    .expect("on-demand plan");

    let plan = plans::create_plan(
        &repository,
        subscription.subscription_id,
        plan_request(product_id, CommercialModel::Free, PlanRecurrence::None, 10),
    )
    .await
    .expect("free plan");
    let revoked = plans::revoke_plan(&repository, plan.plan_version_id, revoke_plan_request())
        .await
        .expect("revoke plan");
    assert!(revoked.revoked_at.is_some());
    assert_audit(
        &pool,
        plan.plan_version_id,
        "subscription_plan.revoked",
        "operator:test",
    )
    .await;
    let plans_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM customer_plans WHERE customer_id=$1")
            .bind(workspace_id)
            .fetch_one(&pool)
            .await
            .expect("plan count before rejected join");
    let join = plans::create_customer_plan(
        &repository,
        workspace_id,
        "revoked-key",
        customer_plan_request(plan.plan_version_id, "revoked-transaction"),
    )
    .await;
    assert_eq!(
        join.expect_err("revoked join").code(),
        "subscription_plan_revoked"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM customer_plans WHERE customer_id=$1")
            .bind(workspace_id)
            .fetch_one(&pool)
            .await
            .expect("plan count"),
        plans_before
    );
    assert!(sqlx::query(
        "UPDATE subscription_plan_versions SET granted_credit_units=999 WHERE plan_version_id=$1"
    )
    .bind(plan.plan_version_id)
    .execute(&pool)
    .await
    .is_err());
    apply_workspace_event(&repository, workspace_id, "workspace.blocked", 3).await;
    assert_blocked_join(&repository, workspace_id, card_plan.plan_version_id).await;
    assert_plan_swagger(&router).await;
}

async fn setup_active_workspace() -> (Router, PgPool, DatabaseRepository, Uuid, Uuid) {
    let (router, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let product_id = create_metered_product(&repository).await;
    let workspace_id = Uuid::new_v4();
    apply_workspace_event(&repository, workspace_id, "workspace.created", 1).await;
    apply_workspace_event(&repository, workspace_id, "workspace.activated", 2).await;
    (router, pool, repository, workspace_id, product_id)
}

async fn create_metered_product(repository: &DatabaseRepository) -> Uuid {
    let product = catalog::create_product(
        repository,
        CreateProductRequest {
            name: format!("Plan product {}", Uuid::new_v4()),
            description: None,
            usage_model: UsageModel::CreditMetered,
        },
    )
    .await
    .expect("product");
    let item = catalog::create_item(
        repository,
        product.product_id,
        CreateItemRequest {
            name: format!("Plan item {}", Uuid::new_v4()),
            parent_item_id: None,
            unit_name: Some("request".to_string()),
            quantity_scale: Some(ItemUnits::positive(1).expect("positive quantity scale")),
        },
    )
    .await
    .expect("item");
    let price = catalog::create_price_version(
        repository,
        item.item_id,
        CreatePriceVersionRequest {
            pricing_model: PricingModel::Unit,
            unit_block_size: Some(ItemUnits::positive(1).expect("positive block size")),
            credit_units: Some(CreditUnits::new(1)),
            effective_from: Utc::now() - Duration::days(1),
            effective_until: None,
            accumulation_cycle: None,
            tiers: Vec::new(),
        },
    )
    .await
    .expect("price");
    catalog::publish_price_version(repository, price.price_version_id)
        .await
        .expect("publish price");
    catalog::update_item(
        repository,
        item.item_id,
        UpdateItemRequest {
            name: None,
            status: Some(CatalogStatus::Active),
            expected_version: 1,
        },
    )
    .await
    .expect("activate item");
    catalog::update_product(
        repository,
        product.product_id,
        UpdateProductRequest {
            name: None,
            description: None,
            status: Some(CatalogStatus::Active),
            expected_version: 1,
        },
    )
    .await
    .expect("activate product");
    product.product_id
}

async fn create_free_plan(
    repository: &DatabaseRepository,
    product_id: Uuid,
    recurrence: PlanRecurrence,
    credits: i64,
) -> subscription::dto::plans::SubscriptionPlanResponse {
    let subscription = plans::create_subscription(repository, subscription_request())
        .await
        .expect("subscription");
    plans::create_plan(
        repository,
        subscription.subscription_id,
        plan_request(product_id, CommercialModel::Free, recurrence, credits),
    )
    .await
    .expect("plan")
}

async fn join_plan(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    plan_id: Uuid,
    suffix: &str,
) -> subscription::dto::plans::CustomerPlanResponse {
    plans::create_customer_plan(
        repository,
        workspace_id,
        &format!("key-{suffix}"),
        customer_plan_request(plan_id, &format!("transaction-{suffix}")),
    )
    .await
    .expect("join plan")
}
#[path = "support/admission_policy_contract.rs"]
mod admission_policy_contract;
#[path = "support/admission_policy_recovery.rs"]
mod admission_policy_recovery;
#[path = "support/plan_phase5_acceptance.rs"]
mod plan_phase5_acceptance;
