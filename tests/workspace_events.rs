mod support;

use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use subscription::{
    repositories::database::DatabaseRepository,
    services::{
        outbox::{dispatch_once, WebhookDestination},
        signatures::{current_timestamp, sign_body, verify_signed_body},
        workspace_events::process_workspace_event,
    },
};
use tower::ServiceExt;

use support::{response_json, setup_router_with_options};

#[derive(Clone)]
struct FakeWebhookState {
    status: StatusCode,
    requests: Arc<tokio::sync::Mutex<Vec<(HeaderMap, bytes::Bytes)>>>,
}

struct FakeWebhookServer {
    url: String,
    requests: Arc<tokio::sync::Mutex<Vec<(HeaderMap, bytes::Bytes)>>>,
    task: tokio::task::JoinHandle<()>,
}

impl FakeWebhookServer {
    async fn start(status: StatusCode) -> Self {
        let requests = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let state = FakeWebhookState {
            status,
            requests: requests.clone(),
        };
        let app = Router::new()
            .route("/events", axum::routing::post(fake_webhook))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake webhook");
        let address = listener.local_addr().expect("fake webhook address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve fake webhook");
        });
        Self {
            url: format!("http://{address}/events"),
            requests,
            task,
        }
    }
}

impl Drop for FakeWebhookServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fake_webhook(
    State(state): State<FakeWebhookState>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> StatusCode {
    state.requests.lock().await.push((headers, body));
    state.status
}

fn signed_workspace_event_request(secret: &str, body: Value) -> Request<Body> {
    let timestamp = current_timestamp().expect("current timestamp");
    let bytes = serde_json::to_vec(&body).expect("serialize workspace event");
    let signature = sign_body(secret, timestamp, &bytes).expect("sign workspace event");
    Request::builder()
        .method(Method::POST)
        .uri("/v1/internal/accounts/workspace-events")
        .header("content-type", "application/json")
        .header("x-runvibe-timestamp", timestamp.to_string())
        .header("x-runvibe-signature", signature)
        .body(Body::from(bytes))
        .expect("build workspace event request")
}

fn workspace_event(
    workspace_id: uuid::Uuid,
    event_id: uuid::Uuid,
    event_type: &str,
    sequence: i64,
) -> Value {
    serde_json::json!({
        "event_id": event_id,
        "event_type": event_type,
        "schema_version": 1,
        "aggregate_id": workspace_id,
        "sequence": sequence,
        "occurred_at": "2026-09-03T12:00:00Z",
        "workspace_id": workspace_id,
        "correlation_id": uuid::Uuid::new_v4(),
        "causation_id": null,
        "payload": { "workspace_id": workspace_id }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_event_foundation_is_atomic_and_idempotent() {
    let secret = "accounts-test-secret";
    let (router, pool) = setup_router_with_options(false, Some(secret.to_string())).await;
    let workspace_id = uuid::Uuid::new_v4();
    let created_id = uuid::Uuid::new_v4();
    let created = workspace_event(workspace_id, created_id, "workspace.created", 1);
    let unsigned = Request::builder()
        .method(Method::POST)
        .uri("/v1/internal/accounts/workspace-events")
        .header("content-type", "application/json")
        .body(Body::from(created.to_string()))
        .expect("build unsigned request");
    let response = router
        .clone()
        .oneshot(unsigned)
        .await
        .expect("request failed");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = router
        .clone()
        .oneshot(signed_workspace_event_request(secret, created.clone()))
        .await
        .expect("request failed");
    assert_eq!(response_json(response).await["outcome"], "applied");
    let duplicate = router
        .clone()
        .oneshot(signed_workspace_event_request(secret, created))
        .await
        .expect("request failed");
    assert_eq!(response_json(duplicate).await["outcome"], "duplicate");
    let gap_id = uuid::Uuid::new_v4();
    let gap = workspace_event(workspace_id, gap_id, "workspace.blocked", 3);
    let response = router
        .clone()
        .oneshot(signed_workspace_event_request(secret, gap))
        .await
        .expect("request failed");
    assert_eq!(response_json(response).await["outcome"], "quarantined");
    let activated = workspace_event(workspace_id, uuid::Uuid::new_v4(), "workspace.activated", 2);
    router
        .clone()
        .oneshot(signed_workspace_event_request(secret, activated))
        .await
        .expect("request failed");
    let replay = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/admin/integration-inbox/{gap_id}/replay"))
                .body(Body::empty())
                .expect("build replay request"),
        )
        .await
        .expect("request failed");
    assert_eq!(response_json(replay).await["workspace_status"], "BLOCKED");
    let stale = workspace_event(workspace_id, uuid::Uuid::new_v4(), "workspace.activated", 2);
    let response = router
        .oneshot(signed_workspace_event_request(secret, stale))
        .await
        .expect("stale event request failed");
    assert_eq!(response_json(response).await["outcome"], "stale");
    assert_workspace_event_counts(&pool, workspace_id).await;
}

async fn assert_workspace_event_counts(pool: &sqlx::PgPool, workspace_id: uuid::Uuid) {
    let row: (String, i64) = sqlx::query_as(
        "SELECT operational_status, external_sequence FROM workspace_projections WHERE workspace_id=$1",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .expect("workspace projection");
    assert_eq!(row, ("BLOCKED".to_string(), 3));
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM integration_inbox WHERE workspace_id=$1), \
         (SELECT count(*) FROM integration_inbox_quarantine q JOIN integration_inbox i \
          ON i.event_id=q.event_id WHERE i.workspace_id=$1), \
         (SELECT count(*) FROM outbox_events WHERE workspace_id=$1)",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .expect("foundation counts");
    assert_eq!(counts, (4, 1, 3));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outbox_retries_dead_letters_and_replays() {
    let secret = "accounts-test-secret";
    let delivery_secret = "delivery-test-secret";
    let (router, pool) = setup_router_with_options(false, Some(secret.to_string())).await;
    let workspace_id = uuid::Uuid::new_v4();
    let created = workspace_event(workspace_id, uuid::Uuid::new_v4(), "workspace.created", 1);
    router
        .clone()
        .oneshot(signed_workspace_event_request(secret, created))
        .await
        .expect("request failed");
    let event_id = sqlx::query_scalar("SELECT event_id FROM outbox_events WHERE workspace_id=$1")
        .bind(workspace_id)
        .fetch_one(&pool)
        .await
        .expect("outbox event");
    fail_until_dead_lettered(&pool, event_id, delivery_secret).await;
    let replay = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/admin/outbox-events/{event_id}/replay"))
                .body(Body::empty())
                .expect("build outbox replay request"),
        )
        .await
        .expect("outbox replay failed");
    assert_eq!(replay.status(), StatusCode::NO_CONTENT);
    deliver_replayed_event(&pool, event_id, delivery_secret).await;
}

async fn fail_until_dead_lettered(pool: &sqlx::PgPool, event_id: uuid::Uuid, secret: &str) {
    let server = FakeWebhookServer::start(StatusCode::INTERNAL_SERVER_ERROR).await;
    dispatch(pool, &server.url, secret).await;
    sqlx::query(
        "UPDATE outbox_events SET delivery_attempts=11,available_at=now() WHERE event_id=$1",
    )
    .bind(event_id)
    .execute(pool)
    .await
    .expect("prepare last attempt");
    dispatch(pool, &server.url, secret).await;
    let dead: bool = sqlx::query_scalar(
        "SELECT dead_lettered_at IS NOT NULL FROM outbox_events WHERE event_id=$1",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .expect("dead-letter state");
    assert!(dead);
}

async fn deliver_replayed_event(pool: &sqlx::PgPool, event_id: uuid::Uuid, secret: &str) {
    let server = FakeWebhookServer::start(StatusCode::NO_CONTENT).await;
    dispatch(pool, &server.url, secret).await;
    let delivered: bool =
        sqlx::query_scalar("SELECT delivered_at IS NOT NULL FROM outbox_events WHERE event_id=$1")
            .bind(event_id)
            .fetch_one(pool)
            .await
            .expect("delivery state");
    assert!(delivered);
    let requests = server.requests.lock().await;
    let (headers, body) = requests.first().expect("captured webhook request");
    let timestamp = headers["x-runvibe-timestamp"]
        .to_str()
        .expect("timestamp header");
    let signature = headers["x-runvibe-signature"]
        .to_str()
        .expect("signature header");
    verify_signed_body(secret, timestamp, signature, body).expect("signature must verify");
}

async fn dispatch(pool: &sqlx::PgPool, url: &str, secret: &str) {
    let destination = WebhookDestination {
        url: url.to_string(),
        secret: secret.to_string(),
    };
    dispatch_once(
        &DatabaseRepository::new(pool.clone()),
        &reqwest::Client::new(),
        &destination,
        uuid::Uuid::new_v4(),
    )
    .await
    .expect("dispatch event");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_workspace_event_delivery_has_one_effect() {
    let (_, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let workspace_id = uuid::Uuid::new_v4();
    let event: subscription::dto::events::WorkspaceEventEnvelope = serde_json::from_value(
        workspace_event(workspace_id, uuid::Uuid::new_v4(), "workspace.created", 1),
    )
    .expect("deserialize workspace event");
    let (first, second) = tokio::join!(
        process_workspace_event(&repository, event.clone()),
        process_workspace_event(&repository, event)
    );
    let outcomes = [
        serde_json::to_value(first.expect("first event").outcome).expect("first outcome"),
        serde_json::to_value(second.expect("second event").outcome).expect("second outcome"),
    ];
    assert!(outcomes.contains(&serde_json::json!("applied")));
    assert!(outcomes.contains(&serde_json::json!("duplicate")));
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workspace_projections WHERE workspace_id=$1), \
         (SELECT count(*) FROM integration_inbox WHERE workspace_id=$1), \
         (SELECT count(*) FROM outbox_events WHERE workspace_id=$1)",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .expect("concurrent event counts");
    assert_eq!(counts, (1, 1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foundation_migration_round_trips() {
    let (_, pool) = setup_router_with_options(false, None).await;
    let mut connection = pool.acquire().await.expect("test connection");
    sqlx::query("DROP SCHEMA IF EXISTS migration_round_trip CASCADE")
        .execute(&mut *connection)
        .await
        .expect("reset isolated schema");
    sqlx::query("CREATE SCHEMA migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("create isolated schema");
    sqlx::query("SET search_path TO migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("select isolated schema");
    sqlx::raw_sql(include_str!("../migrations/202601020000_init.sql"))
        .execute(&mut *connection)
        .await
        .expect("apply template migration");
    sqlx::raw_sql(include_str!("../migrations/202609030001_foundation.up.sql"))
        .execute(&mut *connection)
        .await
        .expect("apply foundation migration");
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('workspace_projections') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await
            .expect("foundation table exists");
    assert!(exists);
    sqlx::raw_sql(include_str!(
        "../migrations/202609030001_foundation.down.sql"
    ))
    .execute(&mut *connection)
    .await
    .expect("revert foundation migration");
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('workspace_projections') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await
            .expect("foundation table removed");
    assert!(!exists);
}
