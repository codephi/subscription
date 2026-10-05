mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use std::sync::Arc;

use chrono::Utc;
use subscription::{
    dto::{
        billing::CreateOnDemandPurchaseRequest,
        plans::{
            AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest, CreateOnDemandPlanRequest,
            CreatePlanTransitionRequest, CreateSubscriptionPlanRequest, CreateSubscriptionRequest,
            PlanRecurrence, PlanTransitionKind, RevokeCustomerPlanRequest, RevokePlanRequest,
            SubscriptionModel,
        },
        units::CreditUnits,
    },
    repositories::{
        billing_confirmation::{ConfirmationResult, ConfirmedBillingWebhook},
        billing_connector::{
            BillingCapabilities, BillingConnector, BillingPaymentMethod, CollectionCommand,
            ConnectorCollectionResult, ConnectorCollectionState, ConnectorFuture,
        },
        database::DatabaseRepository,
    },
    services::{billing, plans},
};
use uuid::Uuid;

use usage_fixture::setup_usage;

struct FakeBillingConnector;

impl BillingConnector for FakeBillingConnector {
    fn capabilities(&self) -> BillingCapabilities {
        fake_capabilities()
    }

    fn start_collection<'a>(&'a self, command: &'a CollectionCommand) -> ConnectorFuture<'a> {
        let provider_payment_id = provider_payment_id(command);
        Box::pin(async move { Ok(pending_result(provider_payment_id)) })
    }
}

struct WebhookBeforeResponseConnector {
    repository: DatabaseRepository,
    webhook: ConfirmedBillingWebhook,
}

impl BillingConnector for WebhookBeforeResponseConnector {
    fn capabilities(&self) -> BillingCapabilities {
        fake_capabilities()
    }

    fn start_collection<'a>(&'a self, command: &'a CollectionCommand) -> ConnectorFuture<'a> {
        let repository = self.repository.clone();
        let webhook = self.webhook.clone();
        let provider_payment_id = provider_payment_id(command);
        Box::pin(async move {
            billing::apply_confirmed_webhook(&repository, &webhook)
                .await
                .expect("webhook before synchronous response");
            Ok(pending_result(provider_payment_id))
        })
    }
}

struct BillingConfirmationFixture {
    repository: DatabaseRepository,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    collection_request_id: Uuid,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn billing_connector_state_machine_is_idempotent() {
    let fixture = setup_confirmation().await;
    let webhook = Arc::new(confirmed_webhook(&fixture));
    let first_repository = fixture.repository.clone();
    let first_webhook = Arc::clone(&webhook);
    let first = tokio::spawn(async move {
        billing::apply_confirmed_webhook(&first_repository, &first_webhook).await
    });
    let second_repository = fixture.repository.clone();
    let second_webhook = Arc::clone(&webhook);
    let second = tokio::spawn(async move {
        billing::apply_confirmed_webhook(&second_repository, &second_webhook).await
    });
    let first = first
        .await
        .expect("first confirmation")
        .expect("apply first");
    let second = second
        .await
        .expect("second confirmation")
        .expect("apply second");
    assert_eq!(
        [first.result, second.result]
            .into_iter()
            .filter(|result| *result == ConfirmationResult::Applied)
            .count(),
        1
    );
    assert_confirmation_effects(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn distinct_duplicate_and_mismatched_confirmation_never_repeat_credit() {
    let fixture = setup_confirmation().await;
    let applied =
        billing::apply_confirmed_webhook(&fixture.repository, &confirmed_webhook(&fixture))
            .await
            .expect("first confirmation");
    assert_eq!(applied.result, ConfirmationResult::Applied);
    let mut duplicate = confirmed_webhook(&fixture);
    duplicate.provider_event_id = format!("different-event-{}", Uuid::new_v4());
    let duplicate = billing::apply_confirmed_webhook(&fixture.repository, &duplicate)
        .await
        .expect("different duplicate event");
    assert_eq!(duplicate.result, ConfirmationResult::Duplicate);
    let mut mismatch = confirmed_webhook(&fixture);
    mismatch.provider_event_id = format!("mismatch-event-{}", Uuid::new_v4());
    mismatch.amount_minor += 1;
    let error = billing::apply_confirmed_webhook(&fixture.repository, &mismatch)
        .await
        .expect_err("mismatched amount");
    assert_eq!(error.code(), "billing_confirmation_mismatch");
    assert_confirmation_effects(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_demand_confirmation_grants_persistent_credit_without_new_cycle_or_plan() {
    let fixture = setup_confirmation().await;
    billing::apply_confirmed_webhook(&fixture.repository, &confirmed_webhook(&fixture))
        .await
        .expect("activate paid plan");
    let (subscription_id, plan_version_id, binding_id): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT sp.subscription_id,cp.plan_version_id,pmb.payment_method_binding_id \
         FROM customer_plans cp JOIN subscription_plan_versions sp USING(plan_version_id) \
         JOIN payment_method_bindings pmb ON pmb.customer_plan_id=cp.customer_plan_id \
         WHERE cp.customer_plan_id=$1",
    )
    .bind(fixture.customer_plan_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .unwrap();
    let offer = plans::create_on_demand_plan(
        &fixture.repository,
        subscription_id,
        CreateOnDemandPlanRequest {
            name: "Extra credits".into(),
            price_amount_minor: 500,
            currency: "BRL".into(),
            credit_units: CreditUnits::new(40),
        },
    )
    .await
    .unwrap();
    let purchase = billing::create_on_demand_purchase(
        &fixture.repository,
        fixture.workspace_id,
        fixture.customer_plan_id,
        &format!("on-demand-key-{}", Uuid::new_v4()),
        &CreateOnDemandPurchaseRequest {
            on_demand_plan_id: offer.on_demand_plan_id,
            quantity: 3,
            payment_method_binding_id: binding_id,
            transaction_id: format!("on-demand-transaction-{}", Uuid::new_v4()),
        },
    )
    .await
    .unwrap();
    billing::execute_collection_attempt(
        &fixture.repository,
        &FakeBillingConnector,
        purchase.collection_request_id,
    )
    .await
    .unwrap()
    .unwrap();
    let webhook = ConfirmedBillingWebhook {
        provider: "FAKE".into(),
        provider_event_id: format!("on-demand-event-{}", Uuid::new_v4()),
        event_type: "payment.confirmed".into(),
        payload_sha256: "c".repeat(64),
        collection_request_id: purchase.collection_request_id,
        provider_payment_id: format!(
            "fake-payment-collection:{}:attempt:1",
            purchase.collection_request_id
        ),
        amount_minor: 1_500,
        currency: "BRL".into(),
        occurred_at: Utc::now(),
    };
    let outcome = billing::apply_confirmed_webhook(&fixture.repository, &webhook)
        .await
        .unwrap();
    assert_eq!(outcome.result, ConfirmationResult::Applied);
    assert!(outcome.customer_plan_cycle_id.is_none());
    let state: (Uuid, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT cp.plan_version_id,cw.balance_credit_units, \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1), \
         (SELECT count(*) FROM credit_lots WHERE customer_id=$2 AND source_kind='ON_DEMAND'), \
         (SELECT count(*) FROM customer_plan_entitlements WHERE customer_plan_id=$1 AND effective_until IS NULL) \
         FROM customer_plans cp JOIN wallets w ON w.customer_id=cp.customer_id AND w.wallet_type='CUSTOMER' \
         JOIN customer_wallets cw ON cw.wallet_id=w.wallet_id WHERE cp.customer_plan_id=$1",
    )
    .bind(fixture.customer_plan_id)
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .unwrap();
    assert_eq!(state, (plan_version_id, 220, 1, 1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paid_upgrade_changes_plan_and_cycle_only_after_full_confirmation() {
    let fixture = setup_confirmation().await;
    billing::apply_confirmed_webhook(&fixture.repository, &confirmed_webhook(&fixture))
        .await
        .unwrap();
    let (subscription_id, product_id, old_plan_id, binding_id): (Uuid, Uuid, Uuid, Uuid) =
        sqlx::query_as(
            "SELECT sp.subscription_id,spp.product_id,cp.plan_version_id,pmb.payment_method_binding_id \
             FROM customer_plans cp JOIN subscription_plan_versions sp USING(plan_version_id) \
             JOIN subscription_plan_products spp USING(plan_version_id) \
             JOIN payment_method_bindings pmb ON pmb.customer_plan_id=cp.customer_plan_id \
             WHERE cp.customer_plan_id=$1",
        )
        .bind(fixture.customer_plan_id)
        .fetch_one(&fixture.repository.pool())
        .await
        .unwrap();
    let target = plans::create_plan(
        &fixture.repository,
        subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Paid upgrade".into(),
            commercial_model: CommercialModel::Paid,
            price_amount_minor: Some(2_000),
            currency: Some("BRL".into()),
            recurrence: PlanRecurrence::Monthly,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: vec!["CARD".into()],
            granted_credit_units: CreditUnits::new(200),
            product_ids: vec![product_id],
        },
    )
    .await
    .unwrap();
    let upgrade = billing::create_paid_plan_upgrade(
        &fixture.repository,
        fixture.workspace_id,
        fixture.customer_plan_id,
        &format!("upgrade-key-{}", Uuid::new_v4()),
        &CreatePlanTransitionRequest {
            new_plan_version_id: target.plan_version_id,
            transition_kind: PlanTransitionKind::Upgrade,
            payment_method_binding_id: Some(binding_id),
            transaction_id: format!("upgrade-transaction-{}", Uuid::new_v4()),
            actor_reference: "customer:test".into(),
        },
    )
    .await
    .unwrap();
    let before: (Uuid, i64, i64) = sqlx::query_as(
        "SELECT cp.plan_version_id,cw.balance_credit_units, \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1) \
         FROM customer_plans cp JOIN wallets w ON w.customer_id=cp.customer_id AND w.wallet_type='CUSTOMER' \
         JOIN customer_wallets cw ON cw.wallet_id=w.wallet_id WHERE cp.customer_plan_id=$1",
    )
    .bind(fixture.customer_plan_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .unwrap();
    assert_eq!(before, (old_plan_id, 100, 1));
    billing::execute_collection_attempt(
        &fixture.repository,
        &FakeBillingConnector,
        upgrade.collection_request_id,
    )
    .await
    .unwrap();
    let webhook = ConfirmedBillingWebhook {
        provider: "FAKE".into(),
        provider_event_id: format!("upgrade-event-{}", Uuid::new_v4()),
        event_type: "payment.confirmed".into(),
        payload_sha256: "d".repeat(64),
        collection_request_id: upgrade.collection_request_id,
        provider_payment_id: format!(
            "fake-payment-collection:{}:attempt:1",
            upgrade.collection_request_id
        ),
        amount_minor: 500,
        currency: "BRL".into(),
        occurred_at: Utc::now(),
    };
    billing::apply_confirmed_webhook(&fixture.repository, &webhook)
        .await
        .unwrap();
    let after: (Uuid, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT cp.plan_version_id,cw.balance_credit_units, \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1), \
         (SELECT count(*) FROM customer_plan_transitions WHERE customer_plan_id=$1 AND transition_kind='UPGRADE'), \
         (SELECT count(*) FROM credit_lots WHERE customer_id=$2 AND source_kind='ON_DEMAND') \
         FROM customer_plans cp JOIN wallets w ON w.customer_id=cp.customer_id AND w.wallet_type='CUSTOMER' \
         JOIN customer_wallets cw ON cw.wallet_id=w.wallet_id WHERE cp.customer_plan_id=$1",
    )
    .bind(fixture.customer_plan_id)
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .unwrap();
    assert_eq!(after, (target.plan_version_id, 200, 2, 1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_and_plan_revocation_terminalize_pending_collections() {
    let canceled = setup_unstarted_confirmation().await;
    plans::cancel_customer_plan(
        &canceled.repository,
        canceled.workspace_id,
        canceled.customer_plan_id,
    )
    .await
    .unwrap();
    let canceled_request: (String, Option<String>) = sqlx::query_as(
        "SELECT status,terminal_reason FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(canceled.collection_request_id)
    .fetch_one(&canceled.repository.pool())
    .await
    .unwrap();
    assert_eq!(
        canceled_request,
        ("CANCELED".into(), Some("CUSTOMER_CANCELED".into()))
    );

    let revoked = setup_unstarted_confirmation().await;
    let plan_id: Uuid =
        sqlx::query_scalar("SELECT plan_version_id FROM customer_plans WHERE customer_plan_id=$1")
            .bind(revoked.customer_plan_id)
            .fetch_one(&revoked.repository.pool())
            .await
            .unwrap();
    plans::revoke_plan(
        &revoked.repository,
        plan_id,
        RevokePlanRequest {
            reason: "offer withdrawn".into(),
            actor_reference: "operator:test".into(),
        },
    )
    .await
    .unwrap();
    let revoked_request: (String, Option<String>) = sqlx::query_as(
        "SELECT status,terminal_reason FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(revoked.collection_request_id)
    .fetch_one(&revoked.repository.pool())
    .await
    .unwrap();
    assert_eq!(
        revoked_request,
        ("CANCELED".into(), Some("PLAN_REVOKED".into()))
    );

    let admin_revoked = setup_unstarted_confirmation().await;
    plans::revoke_customer_plan(
        &admin_revoked.repository,
        admin_revoked.workspace_id,
        admin_revoked.customer_plan_id,
        RevokeCustomerPlanRequest {
            reason: "risk review".into(),
            actor_reference: "operator:risk".into(),
        },
    )
    .await
    .unwrap();
    let admin_request: (String, Option<String>) = sqlx::query_as(
        "SELECT status,terminal_reason FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(admin_revoked.collection_request_id)
    .fetch_one(&admin_revoked.repository.pool())
    .await
    .unwrap();
    assert_eq!(
        admin_request,
        ("CANCELED".into(), Some("CUSTOMER_PLAN_REVOKED".into()))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_billing_snapshot_rolls_back_every_confirmation_effect() {
    let fixture = setup_confirmation().await;
    sqlx::query(
        "UPDATE collection_requests SET granted_credit_units=99 WHERE collection_request_id=$1",
    )
    .bind(fixture.collection_request_id)
    .execute(&fixture.repository.pool())
    .await
    .expect("change billing snapshot");
    let error = billing::apply_confirmed_webhook(&fixture.repository, &confirmed_webhook(&fixture))
        .await
        .expect_err("changed credit snapshot");
    assert_eq!(error.code(), "billing_confirmation_mismatch");
    let effects: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1), \
         (SELECT count(*) FROM customer_plan_entitlements WHERE customer_plan_id=$1), \
         (SELECT balance_credit_units FROM customer_wallets cw JOIN wallets w USING(wallet_id) \
          WHERE w.customer_id=$2 AND w.wallet_type='CUSTOMER')",
    )
    .bind(fixture.customer_plan_id)
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .expect("rolled back confirmation effects");
    assert_eq!(effects, (0, 0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synchronous_response_after_webhook_does_not_regress_confirmation() {
    let fixture = setup_unstarted_confirmation().await;
    let connector = WebhookBeforeResponseConnector {
        repository: fixture.repository.clone(),
        webhook: confirmed_webhook(&fixture),
    };
    billing::execute_collection_attempt(
        &fixture.repository,
        &connector,
        fixture.collection_request_id,
    )
    .await
    .expect("connector execution")
    .expect("started attempt");
    assert_confirmation_effects(&fixture).await;
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM outbox_events WHERE aggregate_type='collection_request' \
         AND aggregate_id=$1 ORDER BY aggregate_sequence",
    )
    .bind(fixture.collection_request_id)
    .fetch_all(&fixture.repository.pool())
    .await
    .expect("ordered collection events");
    assert_eq!(
        events,
        vec!["collection.attempt_requested", "payment.confirmed"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confirmed_webhook_after_commercial_expiration_is_recorded_without_effects() {
    let fixture = setup_confirmation().await;
    let summary =
        billing::expire_collections(&fixture.repository, Utc::now() + chrono::Duration::hours(1))
            .await
            .expect("expire initial payment");
    assert_eq!(summary.expired_requests, 1);
    assert_eq!(summary.canceled_initial_plans, 1);
    let outcome =
        billing::apply_confirmed_webhook(&fixture.repository, &confirmed_webhook(&fixture))
            .await
            .expect("record late webhook");
    assert_eq!(outcome.result, ConfirmationResult::Rejected);
    let state: (String, String, i64, i64, String) = sqlx::query_as(
        "SELECT cp.commercial_status,cr.status, \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1), \
         (SELECT balance_credit_units FROM customer_wallets cw JOIN wallets w USING(wallet_id) \
          WHERE w.customer_id=$2 AND w.wallet_type='CUSTOMER'),wi.result \
         FROM customer_plans cp JOIN collection_requests cr ON cr.customer_plan_id=cp.customer_plan_id \
         JOIN billing_webhook_inbox wi ON wi.provider_event_id LIKE 'event-%' \
         WHERE cp.customer_plan_id=$1 AND cr.collection_request_id=$3",
    )
    .bind(fixture.customer_plan_id)
    .bind(fixture.workspace_id)
    .bind(fixture.collection_request_id)
    .fetch_one(&fixture.repository.pool())
    .await
    .expect("late webhook state");
    assert_eq!(
        state,
        ("CANCELED".into(), "EXPIRED".into(), 0, 0, "REJECTED".into())
    );
}

async fn setup_confirmation() -> BillingConfirmationFixture {
    let fixture = setup_unstarted_confirmation().await;
    billing::execute_collection_attempt(
        &fixture.repository,
        &FakeBillingConnector,
        fixture.collection_request_id,
    )
    .await
    .expect("execute fake collection")
    .expect("new collection attempt");
    fixture
}

async fn setup_unstarted_confirmation() -> BillingConfirmationFixture {
    let usage = setup_usage(1, 1, 0).await;
    let paid_plan = create_paid_plan(&usage.repository, usage.product_id).await;
    let customer_plan = plans::create_customer_plan(
        &usage.repository,
        usage.workspace_id,
        &format!("paid-plan-key-{}", Uuid::new_v4()),
        CreateCustomerPlanRequest {
            plan_version_id: paid_plan.plan_version_id,
            transaction_id: format!("paid-plan-transaction-{}", Uuid::new_v4()),
        },
    )
    .await
    .expect("pending paid customer plan");
    assert_eq!(customer_plan.activation_status, "PENDING_INITIAL_PAYMENT");
    let request_id = insert_billing_records(
        &usage.repository,
        usage.workspace_id,
        customer_plan.customer_plan_id,
        paid_plan.plan_version_id,
    )
    .await;
    BillingConfirmationFixture {
        repository: usage.repository,
        workspace_id: usage.workspace_id,
        customer_plan_id: customer_plan.customer_plan_id,
        collection_request_id: request_id,
    }
}

fn fake_capabilities() -> BillingCapabilities {
    BillingCapabilities {
        payment_methods: vec![BillingPaymentMethod::Card],
        supports_setup_session: true,
        supports_vault: true,
        supports_off_session_charge: true,
        supports_webhook: true,
    }
}

async fn create_paid_plan(
    repository: &DatabaseRepository,
    product_id: Uuid,
) -> subscription::dto::plans::SubscriptionPlanResponse {
    let subscription = plans::create_subscription(
        repository,
        CreateSubscriptionRequest {
            name: format!("Paid subscription {}", Uuid::new_v4()),
            subscription_model: SubscriptionModel::CreditStrict,
        },
    )
    .await
    .expect("paid subscription");
    plans::create_plan(
        repository,
        subscription.subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Paid plan".to_string(),
            commercial_model: CommercialModel::Paid,
            price_amount_minor: Some(1_500),
            currency: Some("BRL".to_string()),
            recurrence: PlanRecurrence::Monthly,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: vec!["CARD".to_string()],
            granted_credit_units: CreditUnits::new(100),
            product_ids: vec![product_id],
        },
    )
    .await
    .expect("paid plan")
}

async fn insert_billing_records(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    plan_version_id: Uuid,
) -> Uuid {
    let pool = repository.pool();
    let connection_id = Uuid::new_v4();
    let binding_id = Uuid::new_v4();
    let request_id = Uuid::new_v4();
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
    .expect("billing connection");
    insert_binding(
        &pool,
        workspace_id,
        customer_plan_id,
        connection_id,
        binding_id,
    )
    .await;
    insert_request(
        &pool,
        workspace_id,
        customer_plan_id,
        plan_version_id,
        binding_id,
        request_id,
    )
    .await;
    request_id
}

async fn insert_binding(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    connection_id: Uuid,
    binding_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO payment_method_bindings (payment_method_binding_id,billing_connection_id, \
         workspace_id,customer_id,customer_plan_id,payment_method, \
         provider_payment_method_reference,status) VALUES ($1,$2,$3,$3,$4,'CARD',$5,'ACTIVE')",
    )
    .bind(binding_id)
    .bind(connection_id)
    .bind(workspace_id)
    .bind(customer_plan_id)
    .bind(format!("pm-{binding_id}"))
    .execute(pool)
    .await
    .expect("payment method binding");
}

async fn insert_request(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    plan_version_id: Uuid,
    binding_id: Uuid,
    request_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id,customer_plan_id, \
         plan_version_id,payment_method_binding_id,request_kind,amount_minor,currency,granted_credit_units, \
         status,transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
         VALUES ($1,$2,$2,$3,$4,$5,'INITIAL',1500,'BRL',100,'SCHEDULED',$6,$7,$8,now(),now()+interval '15 minutes')",
    )
    .bind(request_id)
    .bind(workspace_id)
    .bind(customer_plan_id)
    .bind(plan_version_id)
    .bind(binding_id)
    .bind(format!("billing-transaction-{request_id}"))
    .bind(format!("billing-key-{request_id}"))
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("collection request");
}

fn confirmed_webhook(fixture: &BillingConfirmationFixture) -> ConfirmedBillingWebhook {
    ConfirmedBillingWebhook {
        provider: "FAKE".to_string(),
        provider_event_id: format!("event-{}", Uuid::new_v4()),
        event_type: "payment.confirmed".to_string(),
        payload_sha256: "a".repeat(64),
        collection_request_id: fixture.collection_request_id,
        provider_payment_id: format!(
            "fake-payment-collection:{}:attempt:1",
            fixture.collection_request_id
        ),
        amount_minor: 1_500,
        currency: "BRL".to_string(),
        occurred_at: Utc::now(),
    }
}

async fn assert_confirmation_effects(fixture: &BillingConfirmationFixture) {
    let pool = fixture.repository.pool();
    let state: (String, String, String, i64) = sqlx::query_as(
        "SELECT cp.commercial_status,cp.activation_status,cr.status,cw.balance_credit_units \
         FROM customer_plans cp JOIN collection_requests cr ON cr.customer_plan_id=cp.customer_plan_id \
         JOIN wallets w ON w.customer_id=cp.customer_id AND w.wallet_type='CUSTOMER' \
         JOIN customer_wallets cw ON cw.wallet_id=w.wallet_id \
         WHERE cp.customer_plan_id=$1 AND cr.collection_request_id=$2",
    )
    .bind(fixture.customer_plan_id)
    .bind(fixture.collection_request_id)
    .fetch_one(&pool)
    .await
    .expect("confirmed state");
    assert_eq!(
        state,
        ("ACTIVE_PAID".into(), "ACTIVATED".into(), "PAID".into(), 100)
    );
    let effects: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1), \
         (SELECT count(*) FROM customer_plan_entitlements WHERE customer_plan_id=$1), \
         (SELECT count(*) FROM billing_credit_grant_references WHERE collection_request_id=$2), \
         (SELECT count(*) FROM outbox_events WHERE aggregate_id=$2 AND event_type='payment.confirmed')",
    )
    .bind(fixture.customer_plan_id)
    .bind(fixture.collection_request_id)
    .fetch_one(&pool)
    .await
    .expect("confirmation effects");
    assert_eq!(effects, (1, 1, 1, 1));
    let customer_id: Uuid =
        sqlx::query_scalar("SELECT customer_id FROM customer_plans WHERE customer_plan_id=$1")
            .bind(fixture.customer_plan_id)
            .fetch_one(&pool)
            .await
            .expect("customer id");
    assert_eq!(customer_id, fixture.workspace_id);
}

fn provider_payment_id(command: &CollectionCommand) -> String {
    format!("fake-payment-{}", command.provider_idempotency_key)
}

fn pending_result(provider_payment_id: String) -> ConnectorCollectionResult {
    ConnectorCollectionResult {
        provider_payment_id: Some(provider_payment_id),
        state: ConnectorCollectionState::Pending,
        failure_code: None,
        next_action_url: None,
    }
}
