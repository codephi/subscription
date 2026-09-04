mod support;

use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use subscription::{
    repositories::{
        billing_connector::{
            BillingCapabilities, BillingConnector, BillingConnectorError, BillingPaymentMethod,
            CollectionCommand, ConnectorCollectionResult, ConnectorCollectionState,
            ConnectorFuture,
        },
        database::DatabaseRepository,
    },
    services::billing,
};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Clone)]
enum FakeBehavior {
    Return(ConnectorCollectionResult),
    Reject(BillingConnectorError),
    WaitForRelease(Arc<Notify>),
}

struct FakeBillingConnector {
    behavior: FakeBehavior,
    calls: AtomicUsize,
}

impl FakeBillingConnector {
    fn new(behavior: FakeBehavior) -> Self {
        Self {
            behavior,
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl BillingConnector for FakeBillingConnector {
    fn capabilities(&self) -> BillingCapabilities {
        BillingCapabilities {
            payment_methods: vec![BillingPaymentMethod::Card],
            supports_setup_session: true,
            supports_vault: true,
            supports_off_session_charge: true,
            supports_webhook: true,
        }
    }

    fn start_collection<'a>(&'a self, _: &'a CollectionCommand) -> ConnectorFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let behavior = self.behavior.clone();
        Box::pin(async move {
            match behavior {
                FakeBehavior::Return(result) => Ok(result),
                FakeBehavior::Reject(error) => Err(error),
                FakeBehavior::WaitForRelease(release) => {
                    release.notified().await;
                    Ok(pending_result())
                }
            }
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_workers_start_exactly_one_provider_attempt() {
    let (repository, request_id) = setup_collection_request().await;
    let release = Arc::new(Notify::new());
    let connector = Arc::new(FakeBillingConnector::new(FakeBehavior::WaitForRelease(
        Arc::clone(&release),
    )));
    let worker_repository = repository.clone();
    let worker_connector = Arc::clone(&connector);
    let winner = tokio::spawn(async move {
        billing::execute_collection_attempt(
            &worker_repository,
            worker_connector.as_ref(),
            request_id,
        )
        .await
    });
    wait_for_call(&connector).await;
    let loser = billing::execute_collection_attempt(&repository, connector.as_ref(), request_id)
        .await
        .expect("losing worker");
    assert!(loser.is_none());
    release.notify_one();
    assert!(winner
        .await
        .expect("winner task")
        .expect("winner")
        .is_some());
    assert_eq!(connector.call_count(), 1);
    assert_persisted_state(
        &repository,
        request_id,
        "PENDING_PAYMENT",
        "PENDING",
        "PENDING",
        2,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requires_action_and_definitive_failure_are_normalized() {
    let (repository, request_id) = setup_collection_request().await;
    let action = ConnectorCollectionResult {
        provider_payment_id: Some("fake-action-payment".to_string()),
        state: ConnectorCollectionState::RequiresAction,
        failure_code: None,
        next_action_url: Some("https://billing.invalid/action".to_string()),
    };
    execute_returning(&repository, request_id, action).await;
    assert_persisted_state(
        &repository,
        request_id,
        "PENDING_PAYMENT",
        "REQUIRES_ACTION",
        "REQUIRES_ACTION",
        2,
    )
    .await;

    let (repository, request_id) = setup_collection_request().await;
    let failure = ConnectorCollectionResult {
        provider_payment_id: Some("fake-failed-payment".to_string()),
        state: ConnectorCollectionState::Failed,
        failure_code: Some("card_declined".to_string()),
        next_action_url: None,
    };
    execute_returning(&repository, request_id, failure).await;
    assert_persisted_state(&repository, request_id, "EXHAUSTED", "FAILED", "FAILED", 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_connector_error_preserves_attempt_without_retry() {
    let (repository, request_id) = setup_collection_request().await;
    let connector = FakeBillingConnector::new(FakeBehavior::Reject(BillingConnectorError {
        code: "provider_timeout".to_string(),
        message: "provider outcome is unknown".to_string(),
        retryable: true,
        outcome_uncertain: true,
    }));
    let error = billing::execute_collection_attempt(&repository, &connector, request_id)
        .await
        .expect_err("uncertain connector result");
    assert_eq!(error.code(), "billing_connector_error");
    let repeated = billing::execute_collection_attempt(&repository, &connector, request_id)
        .await
        .expect("repeated worker");
    assert!(repeated.is_none());
    assert_eq!(connector.call_count(), 1);
    assert_persisted_state(
        &repository,
        request_id,
        "PENDING_PAYMENT",
        "UNCERTAIN",
        "PENDING",
        2,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_request_is_rejected_before_attempt_or_provider_call() {
    let (repository, request_id) = setup_collection_request().await;
    sqlx::query(
        "UPDATE collection_requests SET payment_expires_at=scheduled_at+interval '1 microsecond' \
         WHERE collection_request_id=$1",
    )
    .bind(request_id)
    .execute(&repository.pool())
    .await
    .expect("expire collection request");
    let connector = FakeBillingConnector::new(FakeBehavior::Return(pending_result()));
    let error = billing::execute_collection_attempt(&repository, &connector, request_id)
        .await
        .expect_err("expired collection request");
    assert_eq!(error.code(), "collection_request_expired");
    assert_eq!(connector.call_count(), 0);
    let attempts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM collection_attempts WHERE collection_request_id=$1",
    )
    .bind(request_id)
    .fetch_one(&repository.pool())
    .await
    .expect("attempt count");
    assert_eq!(attempts, 0);
}

async fn setup_collection_request() -> (DatabaseRepository, Uuid) {
    let (_, pool) = support::setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let workspace_id = Uuid::new_v4();
    let connection_id = Uuid::new_v4();
    let binding_id = Uuid::new_v4();
    let request_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workspace_projections (workspace_id,operational_status,external_sequence, \
         external_occurred_at,last_event_id) VALUES ($1,'ACTIVE',1,now(),$2)",
    )
    .bind(workspace_id)
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await
    .expect("workspace");
    insert_billing_connection(&pool, workspace_id, connection_id).await;
    insert_payment_binding(&pool, workspace_id, connection_id, binding_id).await;
    insert_collection_request(&pool, workspace_id, binding_id, request_id).await;
    (repository, request_id)
}

async fn insert_billing_connection(pool: &sqlx::PgPool, workspace_id: Uuid, connection_id: Uuid) {
    sqlx::query(
        "INSERT INTO billing_connections (billing_connection_id,workspace_id,provider, \
         external_account_reference,secret_reference,capabilities,status) \
         VALUES ($1,$2,'FAKE','fake-account','secret://fake',ARRAY['CARD'],'ACTIVE')",
    )
    .bind(connection_id)
    .bind(workspace_id)
    .execute(pool)
    .await
    .expect("billing connection");
}

async fn insert_payment_binding(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    connection_id: Uuid,
    binding_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO payment_method_bindings (payment_method_binding_id,billing_connection_id, \
         workspace_id,customer_id,payment_method,provider_payment_method_reference,status) \
         VALUES ($1,$2,$3,$3,'CARD',$4,'ACTIVE')",
    )
    .bind(binding_id)
    .bind(connection_id)
    .bind(workspace_id)
    .bind(format!("pm_{binding_id}"))
    .execute(pool)
    .await
    .expect("payment binding");
}

async fn insert_collection_request(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    binding_id: Uuid,
    request_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id, \
         payment_method_binding_id,request_kind,amount_minor,currency,granted_credit_units,status, \
         transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
         VALUES ($1,$2,$2,$3,'INITIAL',1500,'BRL',100,'SCHEDULED',$4,$5,$6,now(),now()+interval '15 minutes')",
    )
    .bind(request_id)
    .bind(workspace_id)
    .bind(binding_id)
    .bind(format!("transaction-{request_id}"))
    .bind(format!("key-{request_id}"))
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("collection request");
}

async fn execute_returning(
    repository: &DatabaseRepository,
    request_id: Uuid,
    result: ConnectorCollectionResult,
) {
    let connector = FakeBillingConnector::new(FakeBehavior::Return(result));
    billing::execute_collection_attempt(repository, &connector, request_id)
        .await
        .expect("collection execution")
        .expect("started attempt");
    assert_eq!(connector.call_count(), 1);
}

async fn wait_for_call(connector: &FakeBillingConnector) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while connector.call_count() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fake connector call");
}

async fn assert_persisted_state(
    repository: &DatabaseRepository,
    request_id: Uuid,
    request_status: &str,
    attempt_status: &str,
    payment_state: &str,
    event_count: i64,
) {
    let pool = repository.pool();
    let actual: (String, String, String) = sqlx::query_as(
        "SELECT cr.status,ca.status,bp.state FROM collection_requests cr \
         JOIN collection_attempts ca USING(collection_request_id) \
         JOIN billing_payments bp USING(collection_request_id) WHERE cr.collection_request_id=$1",
    )
    .bind(request_id)
    .fetch_one(&pool)
    .await
    .expect("billing state");
    assert_eq!(
        actual,
        (
            request_status.into(),
            attempt_status.into(),
            payment_state.into()
        )
    );
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE aggregate_type='collection_request' AND aggregate_id=$1",
    )
    .bind(request_id)
    .fetch_one(&pool)
    .await
    .expect("billing events");
    assert_eq!(events, event_count);
}

fn pending_result() -> ConnectorCollectionResult {
    ConnectorCollectionResult {
        provider_payment_id: Some("fake-pending-payment".to_string()),
        state: ConnectorCollectionState::Pending,
        failure_code: None,
        next_action_url: None,
    }
}
