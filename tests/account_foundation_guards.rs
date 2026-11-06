mod support;

use axum::{body::Body, http::Request, Router};
use serde_json::{json, Value};
use sqlx::PgPool;
use subscription::{
    dto::events::{AccountEventEnvelope, AccountEventPayload, AccountEventType},
    repositories::database::DatabaseRepository,
    services::{account_events::process_account_event, signatures},
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accounts_invalid_credentials_and_context_leave_foundation_unchanged() {
    let (router, pool) =
        support::setup_router_with_options(false, Some("accounts-secret".into())).await;
    let original = serde_json::to_value(created_event()).unwrap();
    let before = foundation_snapshot(&pool).await;
    assert_rejected_signatures(&router, &original).await;
    for field in ["aggregate_id", "payload"] {
        let mut changed = original.clone();
        changed[field] = if field == "payload" {
            json!({"account_id":Uuid::new_v4()})
        } else {
            json!(Uuid::new_v4())
        };
        let response = send_event(
            &router,
            changed,
            "accounts-secret",
            signatures::current_timestamp().unwrap(),
        )
        .await;
        assert_eq!(response.status(), 422);
        assert_eq!(
            support::response_json(response).await["error"]["code"],
            "account_context_mismatch"
        );
    }
    assert_eq!(foundation_snapshot(&pool).await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_precommit_failure_rolls_back_and_postcommit_retry_has_one_effect() {
    let (_, pool) = support::setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let event = created_event();
    let before = foundation_snapshot(&pool).await;
    install_commit_failure(&pool).await;
    assert!(process_account_event(&repository, event.clone())
        .await
        .is_err());
    assert_eq!(foundation_snapshot(&pool).await, before);
    sqlx::query("DROP TRIGGER fail_account_commit ON audit_events")
        .execute(&pool)
        .await
        .unwrap();
    process_account_event(&repository, event.clone())
        .await
        .expect("retry after rollback");
    let committed = foundation_snapshot(&pool).await;
    assert_ne!(committed, before);
    let duplicate = process_account_event(&repository, event)
        .await
        .expect("response lost after commit");
    assert_eq!(
        serde_json::to_value(duplicate).unwrap()["outcome"],
        "duplicate"
    );
    assert_eq!(foundation_snapshot(&pool).await, committed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_event_identity_cannot_be_reused_with_different_content() {
    let (_, pool) = support::setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let original = created_event();
    process_account_event(&repository, original.clone())
        .await
        .unwrap();
    let committed = foundation_snapshot(&pool).await;
    for changed in conflicting_events(&original) {
        let error = process_account_event(&repository, changed)
            .await
            .expect_err("identity conflict");
        assert_eq!(error.code(), "account_event_identity_conflict");
        assert_eq!(foundation_snapshot(&pool).await, committed);
    }
}

fn conflicting_events(original: &AccountEventEnvelope) -> [AccountEventEnvelope; 2] {
    let mut payload = original.clone();
    payload.event_type = AccountEventType::Activated;
    payload.sequence = 2;
    let mut account = original.clone();
    account.account_id = Uuid::new_v4();
    account.aggregate_id = account.account_id;
    account.payload.account_id = account.account_id;
    [payload, account]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accounts_openapi_declares_signed_event_rejections() {
    let router = support::setup_router().await;
    let response = router
        .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let document = support::response_json(response).await;
    let responses = &document["paths"]["/v1/internal/accounts/account-events"]["post"]["responses"];
    for status in ["202", "401", "409", "422", "503"] {
        assert!(
            responses.get(status).is_some(),
            "missing Accounts response {status}"
        );
    }
}

fn created_event() -> AccountEventEnvelope {
    let account_id = Uuid::new_v4();
    AccountEventEnvelope {
        event_id: Uuid::new_v4(),
        event_type: AccountEventType::Created,
        schema_version: 1,
        aggregate_id: account_id,
        sequence: 1,
        occurred_at: chrono::Utc::now(),
        account_id,
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        payload: AccountEventPayload { account_id },
    }
}

async fn assert_rejected_signatures(router: &Router, event: &Value) {
    let now = signatures::current_timestamp().unwrap();
    for (secret, timestamp) in [("wrong-secret", now), ("accounts-secret", now - 3600)] {
        assert_eq!(
            send_event(router, event.clone(), secret, timestamp)
                .await
                .status(),
            401
        );
    }
    let unsigned = Request::post("/v1/internal/accounts/account-events")
        .header("content-type", "application/json")
        .body(Body::from(event.to_string()))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(unsigned).await.unwrap().status(),
        401
    );
}

async fn send_event(
    router: &Router,
    event: Value,
    secret: &str,
    timestamp: i64,
) -> axum::response::Response {
    let body = event.to_string();
    let signature = signatures::sign_body(secret, timestamp, body.as_bytes()).unwrap();
    let request = Request::post("/v1/internal/accounts/account-events")
        .header("content-type", "application/json")
        .header("x-runvibe-timestamp", timestamp.to_string())
        .header("x-runvibe-signature", signature)
        .body(Body::from(body))
        .unwrap();
    router.clone().oneshot(request).await.unwrap()
}

async fn install_commit_failure(pool: &PgPool) {
    // A deferred trigger fails at COMMIT after inbox, projection, wallet and outbox writes.
    sqlx::raw_sql(
        "CREATE FUNCTION reject_account_commit() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN RAISE EXCEPTION 'injected account commit failure'; END $$; \
        CREATE CONSTRAINT TRIGGER fail_account_commit AFTER INSERT ON audit_events \
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_account_commit();",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn foundation_snapshot(pool: &PgPool) -> Vec<Value> {
    let mut snapshot = Vec::new();
    for table in [
        "account_projections",
        "integration_inbox",
        "integration_inbox_quarantine",
        "outbox_events",
        "audit_events",
        "wallets",
        "customer_wallets",
        "item_wallets",
        "wallet_lifecycle_events",
        "wallet_provisioning",
    ] {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT COALESCE(jsonb_agg(r ORDER BY r::text), '[]') FROM (SELECT to_jsonb(t) r FROM ",
        );
        query.push(table).push(" t) rows");
        snapshot.push(query.build_query_scalar().fetch_one(pool).await.unwrap());
    }
    snapshot
}
