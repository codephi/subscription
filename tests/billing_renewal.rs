mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use subscription::{
    dto::{
        billing::CreateRenewalRegularizationRequest,
        plans::{
            AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest,
            CreateSubscriptionPlanRequest, CreateSubscriptionRequest, PlanRecurrence,
            SubscriptionModel,
        },
        units::CreditUnits,
    },
    repositories::{
        billing_confirmation::{ConfirmationResult, ConfirmedBillingWebhook},
        billing_connector::{ConnectorCollectionResult, ConnectorCollectionState},
        database::DatabaseRepository,
    },
    services::{billing, plans},
};
use uuid::Uuid;

use usage_fixture::setup_usage;

struct PaidRenewalFixture {
    repository: DatabaseRepository,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    plan_version_id: Uuid,
    binding_id: Uuid,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_paid_renewal_preserves_anchor_and_grants_one_cycle_allowance() {
    let fixture = setup_paid_plan().await;
    let initial_request_id = insert_pending_collection(&fixture, "INITIAL", Utc::now()).await;
    let initial_webhook = confirmed_webhook(initial_request_id, Utc::now());
    billing::apply_confirmed_webhook(&fixture.repository, &initial_webhook)
        .await
        .expect("initial confirmation");
    let (anchor_at, first_period_end): (DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "SELECT cp.anchor_at,cy.current_period_end FROM customer_plans cp \
         JOIN customer_plan_cycles cy USING(customer_plan_id) \
         WHERE cp.customer_plan_id=$1 AND cy.status='ACTIVE'",
    )
    .bind(fixture.customer_plan_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .unwrap();
    let renewal_request_id = insert_pending_collection(&fixture, "RENEWAL", first_period_end).await;
    let webhook = Arc::new(confirmed_webhook(renewal_request_id, first_period_end));
    let first = spawn_confirmation(&fixture.repository, Arc::clone(&webhook));
    let second = spawn_confirmation(&fixture.repository, Arc::clone(&webhook));
    let outcomes = [
        first.await.unwrap().unwrap(),
        second.await.unwrap().unwrap(),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.result == ConfirmationResult::Applied)
            .count(),
        1
    );
    assert_renewal_effects(&fixture, renewal_request_id, anchor_at, first_period_end).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manual_regularization_is_idempotent_and_restarts_cycle_only_after_confirmation() {
    let fixture = setup_paid_plan().await;
    let initial_at = Utc::now() - Duration::days(1);
    let initial_request_id = insert_pending_collection(&fixture, "INITIAL", initial_at).await;
    billing::apply_confirmed_webhook(
        &fixture.repository,
        &confirmed_webhook(initial_request_id, initial_at),
    )
    .await
    .unwrap();
    let requested_at = Utc::now();
    sqlx::query(
        "UPDATE customer_plan_cycles SET current_period_end=$2-interval '1 minute' \
         WHERE customer_plan_id=$1 AND status='ACTIVE'",
    )
    .bind(fixture.customer_plan_id)
    .bind(requested_at)
    .execute(&fixture.repository.pool())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE customer_plans SET commercial_status='PAST_DUE',renewal_status='RENEWAL_INACTIVE' \
         WHERE customer_plan_id=$1",
    )
    .bind(fixture.customer_plan_id)
    .execute(&fixture.repository.pool())
    .await
    .unwrap();
    let request = Arc::new(CreateRenewalRegularizationRequest {
        payment_method_binding_id: fixture.binding_id,
        transaction_id: format!("regularization-{}", Uuid::new_v4()),
    });
    let first = spawn_regularization(&fixture, "same-regularization-key", Arc::clone(&request));
    let second = spawn_regularization(&fixture, "same-regularization-key", Arc::clone(&request));
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    assert_eq!(first, second);
    assert_eq!(first.status, "SCHEDULED");
    assert_eq!(
        first.payment_expires_at,
        first.scheduled_at + Duration::minutes(15)
    );
    let mut changed_request = (*request).clone();
    changed_request.payment_method_binding_id = Uuid::new_v4();
    let changed = billing::create_renewal_regularization(
        &fixture.repository,
        fixture.workspace_id,
        fixture.customer_plan_id,
        "same-regularization-key",
        &changed_request,
    )
    .await
    .expect_err("changed request cannot reuse idempotency key");
    assert_eq!(changed.code(), "idempotency_key_already_used");
    assert_preconfirmation_regularization_state(&fixture, first.collection_request_id).await;

    let attempt = fixture
        .repository
        .begin_collection_attempt(first.collection_request_id)
        .await
        .unwrap()
        .unwrap();
    fixture
        .repository
        .record_collection_result(
            &attempt,
            &ConnectorCollectionResult {
                provider_payment_id: Some(format!("payment-{}", first.collection_request_id)),
                state: ConnectorCollectionState::Pending,
                failure_code: None,
                next_action_url: None,
            },
        )
        .await
        .unwrap();
    let confirmed_at = first.scheduled_at + Duration::seconds(1);
    let webhook = Arc::new(confirmed_webhook(first.collection_request_id, confirmed_at));
    let first_confirmation = spawn_confirmation(&fixture.repository, Arc::clone(&webhook));
    let second_confirmation = spawn_confirmation(&fixture.repository, Arc::clone(&webhook));
    let outcomes = [
        first_confirmation.await.unwrap().unwrap(),
        second_confirmation.await.unwrap().unwrap(),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.result == ConfirmationResult::Applied)
            .count(),
        1
    );
    assert_regularization_effects(&fixture, first.collection_request_id, confirmed_at).await;
}

fn spawn_regularization(
    fixture: &PaidRenewalFixture,
    idempotency_key: &str,
    request: Arc<CreateRenewalRegularizationRequest>,
) -> tokio::task::JoinHandle<
    subscription::error::ApiResult<subscription::dto::billing::CollectionRequestResponse>,
> {
    let repository = fixture.repository.clone();
    let workspace_id = fixture.workspace_id;
    let customer_plan_id = fixture.customer_plan_id;
    let idempotency_key = idempotency_key.to_string();
    tokio::spawn(async move {
        billing::create_renewal_regularization(
            &repository,
            workspace_id,
            customer_plan_id,
            &idempotency_key,
            &request,
        )
        .await
    })
}

fn spawn_confirmation(
    repository: &DatabaseRepository,
    webhook: Arc<ConfirmedBillingWebhook>,
) -> tokio::task::JoinHandle<
    subscription::error::ApiResult<
        subscription::repositories::billing_confirmation::ConfirmationOutcome,
    >,
> {
    let repository = repository.clone();
    tokio::spawn(async move { billing::apply_confirmed_webhook(&repository, &webhook).await })
}

async fn setup_paid_plan() -> PaidRenewalFixture {
    let usage = setup_usage(1, 1, 0).await;
    let subscription = plans::create_subscription(
        &usage.repository,
        CreateSubscriptionRequest {
            name: format!("Renewal {}", Uuid::new_v4()),
            subscription_model: SubscriptionModel::CreditStrict,
        },
    )
    .await
    .unwrap();
    let plan = plans::create_plan(
        &usage.repository,
        subscription.subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Paid renewal".into(),
            commercial_model: CommercialModel::Paid,
            price_amount_minor: Some(1_500),
            currency: Some("BRL".into()),
            recurrence: PlanRecurrence::Monthly,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: vec!["CARD".into()],
            granted_credit_units: CreditUnits::new(100),
            product_ids: vec![usage.product_id],
        },
    )
    .await
    .unwrap();
    let customer_plan = plans::create_customer_plan(
        &usage.repository,
        usage.workspace_id,
        &format!("renewal-key-{}", Uuid::new_v4()),
        CreateCustomerPlanRequest {
            plan_version_id: plan.plan_version_id,
            transaction_id: format!("renewal-plan-{}", Uuid::new_v4()),
        },
    )
    .await
    .unwrap();
    let binding_id = insert_binding(
        &usage.repository,
        usage.workspace_id,
        customer_plan.customer_plan_id,
    )
    .await;
    PaidRenewalFixture {
        repository: usage.repository,
        workspace_id: usage.workspace_id,
        customer_plan_id: customer_plan.customer_plan_id,
        plan_version_id: plan.plan_version_id,
        binding_id,
    }
}

async fn insert_binding(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
) -> Uuid {
    let connection_id = Uuid::new_v4();
    let binding_id = Uuid::new_v4();
    let pool = repository.pool();
    sqlx::query(
        "INSERT INTO billing_connections (billing_connection_id,workspace_id,provider, \
         external_account_reference,secret_reference,capabilities,status) \
         VALUES ($1,$2,'FAKE',$3,'secret://fake',ARRAY['CARD'],'ACTIVE')",
    )
    .bind(connection_id)
    .bind(workspace_id)
    .bind(format!("account-{connection_id}"))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO payment_method_bindings (payment_method_binding_id,billing_connection_id, \
         workspace_id,customer_id,customer_plan_id,payment_method,provider_payment_method_reference,status) \
         VALUES ($1,$2,$3,$3,$4,'CARD',$5,'ACTIVE')",
    )
    .bind(binding_id)
    .bind(connection_id)
    .bind(workspace_id)
    .bind(customer_plan_id)
    .bind(format!("pm-{binding_id}"))
    .execute(&pool)
    .await
    .unwrap();
    binding_id
}

async fn insert_pending_collection(
    fixture: &PaidRenewalFixture,
    request_kind: &str,
    occurred_at: DateTime<Utc>,
) -> Uuid {
    let request_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let pool = fixture.repository.pool();
    sqlx::query(
        "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id,customer_plan_id, \
         plan_version_id,payment_method_binding_id,request_kind,amount_minor,currency,granted_credit_units, \
         status,attempts_started,transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
         VALUES ($1,$2,$2,$3,$4,$5,$6,1500,'BRL',100,'PENDING_PAYMENT',1,$7,$8,$9,$10,$11)",
    )
    .bind(request_id)
    .bind(fixture.workspace_id)
    .bind(fixture.customer_plan_id)
    .bind(fixture.plan_version_id)
    .bind(fixture.binding_id)
    .bind(request_kind)
    .bind(format!("transaction-{request_id}"))
    .bind(format!("key-{request_id}"))
    .bind(Uuid::new_v4())
    .bind(occurred_at)
    .bind(occurred_at + Duration::minutes(15))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO collection_attempts (collection_attempt_id,collection_request_id,attempt_number, \
         connector,payment_method,provider_idempotency_key,status,scheduled_at,started_at) \
         VALUES ($1,$2,1,'FAKE','CARD',$3,'PENDING',now(),now())",
    )
    .bind(attempt_id)
    .bind(request_id)
    .bind(format!("attempt-{request_id}"))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO billing_payments (billing_payment_id,collection_request_id,collection_attempt_id, \
         provider,provider_payment_id,state,amount_minor,currency) \
         VALUES ($1,$2,$3,'FAKE',$4,'PENDING',1500,'BRL')",
    )
    .bind(Uuid::new_v4())
    .bind(request_id)
    .bind(attempt_id)
    .bind(format!("payment-{request_id}"))
    .execute(&pool)
    .await
    .unwrap();
    request_id
}

fn confirmed_webhook(request_id: Uuid, occurred_at: DateTime<Utc>) -> ConfirmedBillingWebhook {
    ConfirmedBillingWebhook {
        provider: "FAKE".into(),
        provider_event_id: format!("event-{}", Uuid::new_v4()),
        event_type: "payment.confirmed".into(),
        payload_sha256: "b".repeat(64),
        collection_request_id: request_id,
        provider_payment_id: format!("payment-{request_id}"),
        amount_minor: 1_500,
        currency: "BRL".into(),
        occurred_at,
    }
}

async fn assert_renewal_effects(
    fixture: &PaidRenewalFixture,
    request_id: Uuid,
    anchor_at: DateTime<Utc>,
    first_period_end: DateTime<Utc>,
) {
    let pool = fixture.repository.pool();
    let state: (DateTime<Utc>, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT cp.anchor_at,cw.balance_credit_units, \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1), \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1 AND status='ACTIVE'), \
         (SELECT count(*) FROM customer_wallet_entries WHERE customer_id=$2 AND entry_type='SUBSCRIPTION_CREDIT'), \
         (SELECT count(*) FROM billing_credit_grant_references WHERE collection_request_id=$3) \
         FROM customer_plans cp JOIN wallets w ON w.customer_id=cp.customer_id AND w.wallet_type='CUSTOMER' \
         JOIN customer_wallets cw USING(wallet_id) WHERE cp.customer_plan_id=$1",
    )
    .bind(fixture.customer_plan_id)
    .bind(fixture.workspace_id)
    .bind(request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, (anchor_at, 100, 2, 1, 2, 1));
    let second_start: DateTime<Utc> = sqlx::query_scalar(
        "SELECT current_period_start FROM customer_plan_cycles \
         WHERE customer_plan_id=$1 AND cycle_ordinal=2",
    )
    .bind(fixture.customer_plan_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(second_start, first_period_end);
}

async fn assert_preconfirmation_regularization_state(
    fixture: &PaidRenewalFixture,
    request_id: Uuid,
) {
    let state: (String, String, i64, i64) = sqlx::query_as(
        "SELECT cp.commercial_status,cp.renewal_status, \
         (SELECT count(*) FROM collection_requests WHERE customer_plan_id=$1 \
          AND request_kind='RENEWAL_REGULARIZATION'),cw.balance_credit_units \
         FROM customer_plans cp JOIN wallets w ON w.customer_id=cp.customer_id \
           AND w.wallet_type='CUSTOMER' JOIN customer_wallets cw USING(wallet_id) \
         WHERE cp.customer_plan_id=$1 AND EXISTS(SELECT 1 FROM collection_requests \
           WHERE collection_request_id=$2 AND status='SCHEDULED')",
    )
    .bind(fixture.customer_plan_id)
    .bind(request_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .unwrap();
    assert_eq!(
        state,
        ("PAST_DUE".into(), "RENEWAL_INACTIVE".into(), 1, 100)
    );
}

async fn assert_regularization_effects(
    fixture: &PaidRenewalFixture,
    request_id: Uuid,
    confirmed_at: DateTime<Utc>,
) {
    let state: (String, String, DateTime<Utc>, i64, i64, i64, String) = sqlx::query_as(
        "SELECT cp.commercial_status,cp.renewal_status,cp.anchor_at, \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1), \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1 AND status='ACTIVE'), \
         cw.balance_credit_units,cr.status FROM customer_plans cp \
         JOIN wallets w ON w.customer_id=cp.customer_id AND w.wallet_type='CUSTOMER' \
         JOIN customer_wallets cw USING(wallet_id) JOIN collection_requests cr \
           ON cr.customer_plan_id=cp.customer_plan_id AND cr.collection_request_id=$2 \
         WHERE cp.customer_plan_id=$1",
    )
    .bind(fixture.customer_plan_id)
    .bind(request_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .unwrap();
    assert_eq!(
        state,
        (
            "ACTIVE_PAID".into(),
            "CURRENT".into(),
            confirmed_at,
            2,
            1,
            100,
            "PAID".into()
        )
    );
    let period_start: DateTime<Utc> = sqlx::query_scalar(
        "SELECT current_period_start FROM customer_plan_cycles \
         WHERE customer_plan_id=$1 AND status='ACTIVE'",
    )
    .bind(fixture.customer_plan_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .unwrap();
    assert_eq!(period_start, confirmed_at);
}
