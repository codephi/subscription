mod support;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use subscription::{
    dto::{credits::DirectCreditRequest, events::AccountEventEnvelope},
    repositories::database::DatabaseRepository,
    services::{account_events::process_account_event, credits},
};
use tower::ServiceExt;
use uuid::Uuid;

use support::{response_bytes, response_json, setup_router_with_options};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn customer_wallet_ledger_is_atomic_and_idempotent() {
    let (router, pool, account_id) = active_account().await;
    let response = direct_credit(
        &router,
        account_id,
        "credit-key-1",
        credit_body("credit-transaction-1", "100", json!({"order":"one"})),
    )
    .await;
    if response.status() != StatusCode::CREATED {
        panic!("direct credit failed: {}", response_json(response).await);
    }
    let granted = response_json(response).await;
    assert_eq!(granted["entry"]["balance_before_credit_units"], "0");
    assert_eq!(granted["entry"]["balance_after_credit_units"], "100");
    assert_eq!(granted["entry"]["metadata"]["order"], "one");
    assert_eq!(
        granted["entry"]["references"]
            .as_array()
            .expect("references")
            .len(),
        3
    );

    let duplicate = direct_credit(
        &router,
        account_id,
        "credit-key-1",
        credit_body("different-transaction", "999", json!({"changed":true})),
    )
    .await;
    assert_conflict(duplicate, "idempotency_key_already_used").await;
    let duplicate_transaction = direct_credit(
        &router,
        account_id,
        "credit-key-2",
        credit_body("credit-transaction-1", "100", json!({})),
    )
    .await;
    assert_conflict(duplicate_transaction, "transaction_already_exists").await;

    assert_ledger_counts(&pool, account_id, 1, 100).await;
    assert_transaction_lookup(&router, account_id, "credit-transaction-1").await;
    assert_reconciliation(&router, account_id, true).await;
    assert_credit_history_is_append_only(&pool, account_id).await;
    assert_credit_swagger(&router).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_transaction_id_has_one_effect() {
    let (_, pool, account_id) = active_account().await;
    let first_repository = DatabaseRepository::new(pool.clone());
    let second_repository = DatabaseRepository::new(pool.clone());
    let first_request = credit_request("shared-transaction", 40);
    let second_request = credit_request("shared-transaction", 40);
    let (first, second) = tokio::join!(
        credits::grant_direct_credit(
            &first_repository,
            account_id,
            "concurrent-key-1",
            first_request
        ),
        credits::grant_direct_credit(
            &second_repository,
            account_id,
            "concurrent-key-2",
            second_request
        )
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let error = first.err().or_else(|| second.err()).expect("one conflict");
    assert_eq!(error.code(), "transaction_already_exists");
    assert_ledger_counts(&pool, account_id, 1, 40).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_idempotency_key_has_one_effect() {
    let (_, pool, account_id) = active_account().await;
    let first_repository = DatabaseRepository::new(pool.clone());
    let second_repository = DatabaseRepository::new(pool.clone());
    let (first, second) = tokio::join!(
        credits::grant_direct_credit(
            &first_repository,
            account_id,
            "shared-idempotency-key",
            credit_request("idempotency-transaction-1", 25)
        ),
        credits::grant_direct_credit(
            &second_repository,
            account_id,
            "shared-idempotency-key",
            credit_request("idempotency-transaction-2", 75)
        )
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let error = first.err().or_else(|| second.err()).expect("one conflict");
    assert_eq!(error.code(), "idempotency_key_already_used");
    let balance: i64 = sqlx::query_scalar(
        "SELECT cw.balance_credit_units FROM customer_wallets cw JOIN wallets w ON w.wallet_id=cw.wallet_id \
         WHERE w.customer_id=$1",
    )
    .bind(account_id)
    .fetch_one(&pool)
    .await
    .expect("wallet balance");
    assert!(balance == 25 || balance == 75);
    assert_ledger_counts(&pool, account_id, 1, balance).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transaction_failure_rolls_back_all_effects() {
    let (router, pool, account_id) = active_account().await;
    let initial = direct_credit(
        &router,
        account_id,
        "initial-key",
        credit_body("initial-transaction", "1", json!({})),
    )
    .await;
    assert_eq!(initial.status(), StatusCode::CREATED);
    let overflow = direct_credit(
        &router,
        account_id,
        "overflow-key",
        credit_body("overflow-transaction", "9223372036854775807", json!({})),
    )
    .await;
    assert_eq!(overflow.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response_json(overflow).await["error"]["code"],
        "credit_units_overflow"
    );
    assert_ledger_counts(&pool, account_id, 1, 1).await;
    let reservations: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM idempotency_records WHERE account_id=$1 AND idempotency_key='overflow-key'), \
         (SELECT count(*) FROM transaction_reservations WHERE account_id=$1 AND transaction_id='overflow-transaction')",
    )
    .bind(account_id)
    .fetch_one(&pool)
    .await
    .expect("rolled back reservations");
    assert_eq!(reservations, (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_commit_retry_finds_original_transaction() {
    let (router, _, account_id) = active_account().await;
    let first = direct_credit(
        &router,
        account_id,
        "post-commit-key",
        credit_body("post-commit-transaction", "15", json!({"attempt":1})),
    )
    .await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let retry = direct_credit(
        &router,
        account_id,
        "post-commit-key-retry",
        credit_body("post-commit-transaction", "15", json!({"attempt":1})),
    )
    .await;
    assert_conflict(retry, "transaction_already_exists").await;
    assert_transaction_lookup(&router, account_id, "post-commit-transaction").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn statement_paginates_without_offset_and_account_state_blocks_credit() {
    let (router, pool, account_id) = active_account().await;
    for index in 1..=3 {
        let response = direct_credit(
            &router,
            account_id,
            &format!("page-key-{index}"),
            credit_body(&format!("page-transaction-{index}"), "10", json!({})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    let first_page = get_json(
        &router,
        &format!("/v1/accounts/{account_id}/customer-wallet/statement?limit=2"),
    )
    .await;
    assert_eq!(first_page["items"].as_array().expect("entries").len(), 2);
    let cursor = first_page["next_cursor"].as_str().expect("next cursor");
    let second_page = get_json(
        &router,
        &format!("/v1/accounts/{account_id}/customer-wallet/statement?limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(second_page["items"].as_array().expect("entries").len(), 1);
    assert!(second_page["next_cursor"].is_null());

    let disabled = put_json(
        &router,
        &format!("/v1/accounts/{account_id}/billing-config"),
        json!({
            "direct_credit_enabled":false,"recurring_credit_enabled":true,"expected_version":1
        }),
    )
    .await;
    assert_eq!(disabled["direct_credit_enabled"], false);
    let disabled_credit = direct_credit(
        &router,
        account_id,
        "disabled-key",
        credit_body("disabled-transaction", "10", json!({})),
    )
    .await;
    assert_conflict(disabled_credit, "direct_credit_disabled").await;
    put_json(
        &router,
        &format!("/v1/accounts/{account_id}/billing-config"),
        json!({
            "direct_credit_enabled":true,"recurring_credit_enabled":true,"expected_version":2
        }),
    )
    .await;

    let repository = DatabaseRepository::new(pool.clone());
    apply_account_event(&repository, account_id, "account.blocked", 3).await;
    let blocked = direct_credit(
        &router,
        account_id,
        "blocked-key",
        credit_body("blocked-transaction", "10", json!({})),
    )
    .await;
    assert_conflict(blocked, "account_not_operational").await;
    assert_ledger_counts(&pool, account_id, 3, 30).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_ledger_migration_round_trips() {
    let (_, pool, _) = active_account().await;
    let mut connection = pool.acquire().await.expect("test connection");
    sqlx::query("DROP SCHEMA IF EXISTS credit_migration_round_trip CASCADE")
        .execute(&mut *connection)
        .await
        .expect("reset isolated schema");
    sqlx::query("CREATE SCHEMA credit_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("create isolated schema");
    sqlx::query("SET search_path TO credit_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("select isolated schema");
    for migration in [
        include_str!("../migrations/202601020000_init.sql"),
        include_str!("../migrations/202609030001_foundation.up.sql"),
        include_str!("../migrations/202609030002_catalog.up.sql"),
        include_str!("../migrations/202609030003_wallet_provisioning.up.sql"),
        include_str!("../migrations/202609030004_credit_ledger.up.sql"),
    ] {
        apply_migration(&mut connection, migration).await;
    }
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('customer_wallet_entries') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await
            .expect("ledger table exists");
    assert!(exists);
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609030004_credit_ledger.down.sql"),
    )
    .await;
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('customer_wallet_entries') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await
            .expect("ledger table removed");
    assert!(!exists);
}

async fn active_account() -> (Router, sqlx::PgPool, Uuid) {
    let (router, pool) = setup_router_with_options(false, None).await;
    let account_id = Uuid::new_v4();
    let repository = DatabaseRepository::new(pool.clone());
    apply_account_event(&repository, account_id, "account.created", 1).await;
    apply_account_event(&repository, account_id, "account.activated", 2).await;
    (router, pool, account_id)
}

async fn apply_account_event(
    repository: &DatabaseRepository,
    account_id: Uuid,
    kind: &str,
    sequence: i64,
) {
    let event: AccountEventEnvelope = serde_json::from_value(json!({
        "event_id":Uuid::new_v4(),"event_type":kind,"schema_version":1,"aggregate_id":account_id,
        "sequence":sequence,"occurred_at":"2026-09-04T00:00:00Z","account_id":account_id,
        "correlation_id":Uuid::new_v4(),"causation_id":null,"payload":{"account_id":account_id}
    }))
    .expect("account event");
    process_account_event(repository, event)
        .await
        .expect("apply account event");
}

fn credit_request(transaction_id: &str, units: i64) -> DirectCreditRequest {
    serde_json::from_value(credit_body(transaction_id, &units.to_string(), json!({})))
        .expect("credit request")
}

fn credit_body(transaction_id: &str, units: &str, metadata: Value) -> Value {
    json!({
        "transaction_id":transaction_id,"credit_units":units,"external_reference":"order-123",
        "description":"Direct credit test","metadata":metadata
    })
}

async fn direct_credit(
    router: &Router,
    account_id: Uuid,
    key: &str,
    body: Value,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/accounts/{account_id}/credits/direct"))
                .header("content-type", "application/json")
                .header("idempotency-key", key)
                .body(Body::from(body.to_string()))
                .expect("credit request"),
        )
        .await
        .expect("credit response")
}

async fn assert_conflict(response: axum::response::Response, code: &str) {
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response_json(response).await["error"]["code"], code);
}

async fn assert_ledger_counts(pool: &sqlx::PgPool, account_id: Uuid, entries: i64, balance: i64) {
    let values: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM customer_wallet_entries WHERE customer_id=$1), \
         (SELECT count(*) FROM credit_lots WHERE customer_id=$1), \
         (SELECT count(*) FROM direct_credits WHERE customer_id=$1), \
         (SELECT cw.balance_credit_units FROM customer_wallets cw JOIN wallets w ON w.wallet_id=cw.wallet_id \
          WHERE w.customer_id=$1)",
    ).bind(account_id).fetch_one(pool).await.expect("ledger state");
    assert_eq!(values, (entries, entries, entries, balance));
}

async fn assert_transaction_lookup(router: &Router, account_id: Uuid, transaction_id: &str) {
    let entry = get_json(
        router,
        &format!("/v1/accounts/{account_id}/customer-wallet/transactions/{transaction_id}"),
    )
    .await;
    assert_eq!(entry["transaction_id"], transaction_id);
}

async fn assert_reconciliation(router: &Router, account_id: Uuid, consistent: bool) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/accounts/{account_id}/customer-wallet/reconcile"
                ))
                .body(Body::empty())
                .expect("reconcile request"),
        )
        .await
        .expect("reconcile response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["consistent"], consistent);
}

async fn assert_credit_history_is_append_only(pool: &sqlx::PgPool, account_id: Uuid) {
    let entry_id: Uuid = sqlx::query_scalar(
        "SELECT customer_wallet_entry_id FROM customer_wallet_entries WHERE customer_id=$1",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await
    .expect("entry id");
    assert!(sqlx::query("UPDATE customer_wallet_entries SET description='changed' WHERE customer_wallet_entry_id=$1")
        .bind(entry_id).execute(pool).await.is_err());
    assert!(sqlx::query(
        "DELETE FROM wallet_transaction_references WHERE customer_wallet_entry_id=$1"
    )
    .bind(entry_id)
    .execute(pool)
    .await
    .is_err());
}

async fn assert_credit_swagger(router: &Router) {
    let openapi = get_json(router, "/openapi.json").await;
    assert!(openapi["paths"]
        .get("/v1/accounts/{account_id}/credits/direct")
        .is_some());
    assert!(openapi["components"]["schemas"]
        .get("DirectCreditRequest")
        .is_some());
    let docs = get_response(router, "/docs/").await;
    assert_eq!(docs.status(), StatusCode::OK);
    assert!(String::from_utf8_lossy(&response_bytes(docs).await).contains("Swagger UI"));
}

async fn get_json(router: &Router, uri: &str) -> Value {
    let response = get_response(router, uri).await;
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn get_response(router: &Router, uri: &str) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .body(Body::empty())
                .expect("get request"),
        )
        .await
        .expect("get response")
}

async fn put_json(router: &Router, uri: &str, body: Value) -> Value {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("put request"),
        )
        .await
        .expect("put response");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn apply_migration(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    sql: &'static str,
) {
    sqlx::raw_sql(sql)
        .execute(&mut **connection)
        .await
        .expect("apply migration");
}
