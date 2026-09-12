mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use chrono::{Duration, Utc};
use subscription::{
    dto::{
        plans::{
            AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest, CreateOnDemandPlanRequest,
            CreateSubscriptionPlanRequest, CreateSubscriptionRequest, PlanRecurrence,
            SubscriptionModel,
        },
        units::CreditUnits,
    },
    services::{billing, plans},
};
use uuid::Uuid;

use usage_fixture::setup_usage;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expiration_is_idempotent_and_applies_only_kind_specific_plan_effects() {
    let usage = setup_usage(1, 1, 10).await;
    let pool = usage.repository.pool();
    let binding_id = insert_billing_binding(&pool, usage.workspace_id).await;
    let active_plan_id: Uuid =
        sqlx::query_scalar("SELECT plan_version_id FROM customer_plans WHERE customer_plan_id=$1")
            .bind(usage.customer_plan_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let subscription_id: Uuid = sqlx::query_scalar(
        "SELECT subscription_id FROM subscription_plan_versions WHERE plan_version_id=$1",
    )
    .bind(active_plan_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let on_demand = plans::create_on_demand_plan(
        &usage.repository,
        subscription_id,
        CreateOnDemandPlanRequest {
            name: "Extra credits".into(),
            price_amount_minor: 500,
            currency: "BRL".into(),
            credit_units: CreditUnits::new(25),
        },
    )
    .await
    .unwrap();
    insert_due_request(
        &pool,
        usage.workspace_id,
        usage.customer_plan_id,
        None,
        Some(on_demand.on_demand_plan_id),
        binding_id,
        "ON_DEMAND",
    )
    .await;
    let (initial_customer_plan, initial_plan) =
        create_paid_customer_plan(&usage.repository, usage.workspace_id, usage.product_id).await;
    insert_due_request(
        &pool,
        usage.workspace_id,
        initial_customer_plan,
        Some(initial_plan),
        None,
        binding_id,
        "INITIAL",
    )
    .await;
    let (renewal_customer_plan, renewal_plan) =
        create_paid_customer_plan(&usage.repository, usage.workspace_id, usage.product_id).await;
    sqlx::query(
        "UPDATE customer_plans SET commercial_status='ACTIVE_PAID',activation_status='ACTIVATED' \
         WHERE customer_plan_id=$1",
    )
    .bind(renewal_customer_plan)
    .execute(&pool)
    .await
    .unwrap();
    insert_due_request(
        &pool,
        usage.workspace_id,
        renewal_customer_plan,
        Some(renewal_plan),
        None,
        binding_id,
        "RENEWAL",
    )
    .await;
    let summary = billing::expire_collections(&usage.repository, Utc::now())
        .await
        .unwrap();
    assert_eq!(
        (
            summary.expired_requests,
            summary.canceled_initial_plans,
            summary.past_due_renewals
        ),
        (3, 1, 1)
    );
    assert_plan_status(&pool, usage.customer_plan_id, "ACTIVE", "CURRENT").await;
    assert_plan_status(&pool, initial_customer_plan, "CANCELED", "RENEWAL_INACTIVE").await;
    assert_plan_status(&pool, renewal_customer_plan, "PAST_DUE", "RENEWAL_INACTIVE").await;
    let effects: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM collection_requests WHERE status='EXPIRED' \
         AND terminal_reason='PAYMENT_WINDOW_EXPIRED'), \
         (SELECT count(*) FROM outbox_events WHERE event_type='collection.expired'), \
         (SELECT balance_credit_units FROM customer_wallets cw JOIN wallets w USING(wallet_id) \
          WHERE w.customer_id=$1 AND w.wallet_type='CUSTOMER')",
    )
    .bind(usage.workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(effects, (3, 3, 10));
    assert_eq!(
        billing::expire_collections(&usage.repository, Utc::now())
            .await
            .unwrap()
            .expired_requests,
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn definitive_renewal_failure_preserves_credit_and_creates_no_follow_up_effects() {
    let usage = setup_usage(1, 1, 10).await;
    let pool = usage.repository.pool();
    let binding_id = insert_billing_binding(&pool, usage.workspace_id).await;
    let (customer_plan_id, plan_version_id) =
        create_paid_customer_plan(&usage.repository, usage.workspace_id, usage.product_id).await;
    sqlx::query(
        "UPDATE customer_plans SET commercial_status='ACTIVE_PAID',activation_status='ACTIVATED' \
         WHERE customer_plan_id=$1",
    )
    .bind(customer_plan_id)
    .execute(&pool)
    .await
    .unwrap();
    let request_id = insert_due_request(
        &pool,
        usage.workspace_id,
        customer_plan_id,
        Some(plan_version_id),
        None,
        binding_id,
        "RENEWAL",
    )
    .await;
    sqlx::query(
        "UPDATE collection_requests SET status='SCHEDULED',scheduled_at=now(), \
         payment_expires_at=now()+interval '15 minutes' WHERE collection_request_id=$1",
    )
    .bind(request_id)
    .execute(&pool)
    .await
    .unwrap();
    let attempt = usage
        .repository
        .begin_collection_attempt(request_id)
        .await
        .unwrap()
        .unwrap();
    let failure = subscription::repositories::billing_connector::ConnectorCollectionResult {
        provider_payment_id: Some("failed-renewal".into()),
        state: subscription::repositories::billing_connector::ConnectorCollectionState::Failed,
        failure_code: Some("card_declined".into()),
        next_action_url: None,
    };
    usage
        .repository
        .record_collection_result(&attempt, &failure)
        .await
        .unwrap();
    usage
        .repository
        .record_collection_result(&attempt, &failure)
        .await
        .unwrap();
    assert_plan_status(&pool, customer_plan_id, "PAST_DUE", "RENEWAL_INACTIVE").await;
    let effects: (String, Option<String>, i64, i64, i64) = sqlx::query_as(
        "SELECT status,terminal_reason, \
         (SELECT count(*) FROM collection_requests WHERE customer_plan_id=$2), \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$2), \
         (SELECT balance_credit_units FROM customer_wallets cw JOIN wallets w USING(wallet_id) \
          WHERE w.customer_id=$3 AND w.wallet_type='CUSTOMER') \
         FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(request_id)
    .bind(customer_plan_id)
    .bind(usage.workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        effects,
        ("EXHAUSTED".into(), Some("card_declined".into()), 1, 0, 10)
    );
}

async fn insert_billing_binding(pool: &sqlx::PgPool, workspace_id: Uuid) -> Uuid {
    let connection_id = Uuid::new_v4();
    let binding_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO billing_connections (billing_connection_id,workspace_id,provider, \
         external_account_reference,secret_reference,capabilities,status) \
         VALUES ($1,$2,'FAKE',$3,'secret://fake',ARRAY['CARD'],'ACTIVE')",
    )
    .bind(connection_id)
    .bind(workspace_id)
    .bind(format!("account-{connection_id}"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO payment_method_bindings (payment_method_binding_id,billing_connection_id, \
         workspace_id,customer_id,payment_method,provider_payment_method_reference,status) \
         VALUES ($1,$2,$3,$3,'CARD',$4,'ACTIVE')",
    )
    .bind(binding_id)
    .bind(connection_id)
    .bind(workspace_id)
    .bind(format!("pm-{binding_id}"))
    .execute(pool)
    .await
    .unwrap();
    binding_id
}

#[allow(clippy::too_many_arguments)]
async fn insert_due_request(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    plan_version_id: Option<Uuid>,
    on_demand_plan_id: Option<Uuid>,
    binding_id: Uuid,
    kind: &str,
) -> Uuid {
    let request_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id, \
         customer_plan_id,plan_version_id,on_demand_plan_id,payment_method_binding_id,request_kind, \
         amount_minor,currency,granted_credit_units,status,transaction_id,idempotency_key,correlation_id, \
         scheduled_at,payment_expires_at) VALUES ($1,$2,$2,$3,$4,$5,$6,$7,500,'BRL',25, \
         'PENDING_PAYMENT',$8,$9,$10,$11,$12)",
    )
    .bind(request_id)
    .bind(workspace_id)
    .bind(customer_plan_id)
    .bind(plan_version_id)
    .bind(on_demand_plan_id)
    .bind(binding_id)
    .bind(kind)
    .bind(format!("transaction-{request_id}"))
    .bind(format!("key-{request_id}"))
    .bind(Uuid::new_v4())
    .bind(Utc::now() - Duration::minutes(2))
    .bind(Utc::now() - Duration::minutes(1))
    .execute(pool)
    .await
    .unwrap();
    request_id
}

async fn create_paid_customer_plan(
    repository: &subscription::repositories::database::DatabaseRepository,
    workspace_id: Uuid,
    product_id: Uuid,
) -> (Uuid, Uuid) {
    let subscription = plans::create_subscription(
        repository,
        CreateSubscriptionRequest {
            name: format!("Paid {}", Uuid::new_v4()),
            subscription_model: SubscriptionModel::CreditStrict,
        },
    )
    .await
    .unwrap();
    let plan = plans::create_plan(
        repository,
        subscription.subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Paid plan".into(),
            commercial_model: CommercialModel::Paid,
            price_amount_minor: Some(500),
            currency: Some("BRL".into()),
            recurrence: PlanRecurrence::Monthly,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: vec!["CARD".into()],
            granted_credit_units: CreditUnits::new(25),
            product_ids: vec![product_id],
        },
    )
    .await
    .unwrap();
    let customer_plan = plans::create_customer_plan(
        repository,
        workspace_id,
        &format!("paid-key-{}", Uuid::new_v4()),
        CreateCustomerPlanRequest {
            plan_version_id: plan.plan_version_id,
            transaction_id: format!("paid-transaction-{}", Uuid::new_v4()),
        },
    )
    .await
    .unwrap();
    (customer_plan.customer_plan_id, plan.plan_version_id)
}

async fn assert_plan_status(
    pool: &sqlx::PgPool,
    customer_plan_id: Uuid,
    commercial: &str,
    renewal: &str,
) {
    let state: (String, String) = sqlx::query_as(
        "SELECT commercial_status,renewal_status FROM customer_plans WHERE customer_plan_id=$1",
    )
    .bind(customer_plan_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(state, (commercial.into(), renewal.into()));
}
