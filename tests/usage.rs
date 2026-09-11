mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use std::sync::OnceLock;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use subscription::services::usage;
use subscription::{
    dto::catalog::AccumulationCycleInput, services::calendar::pricing_cycle_bounds,
};
use tokio::sync::Mutex;
use tower::ServiceExt;

use support::response_json;
use usage_fixture::{
    setup_tiered_usage, setup_usage, setup_versioned_usage, standard_tiers, usage_request,
};

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_unit_conversion_preserves_pending_and_debits_exactly() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let fixture = setup_usage(1_000, 100, 500).await;
    let first = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "usage-key-1",
        usage_request(&fixture, "usage-transaction-1", 1_012),
    )
    .await
    .expect("first usage");
    assert_eq!(first.converted_item_units.value(), 1_000);
    assert_eq!(first.pending_item_units_after.value(), 12);
    assert_eq!(first.debited_credit_units.value(), 100);
    assert_eq!(
        first.balance_after_credit_units.expect("balance").value(),
        400
    );

    let second = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "usage-key-2",
        usage_request(&fixture, "usage-transaction-2", 988),
    )
    .await
    .expect("second usage");
    assert_eq!(second.pending_item_units_before.value(), 12);
    assert_eq!(second.pending_item_units_after.value(), 0);
    assert_eq!(
        second.balance_after_credit_units.expect("balance").value(),
        300
    );
    assert_usage_state(
        &fixture.pool,
        fixture.workspace_id,
        fixture.item_id,
        ExpectedUsageState::new(2_000, 2_000, 0, 2, 2),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_partial_usage_forms_one_block() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let fixture = setup_usage(1_000, 100, 500).await;
    let first_repository = fixture.repository.clone();
    let second_repository = fixture.repository.clone();
    let (first, second) = tokio::join!(
        usage::record_usage(
            &first_repository,
            fixture.workspace_id,
            "concurrent-usage-1",
            usage_request(&fixture, "concurrent-transaction-1", 600)
        ),
        usage::record_usage(
            &second_repository,
            fixture.workspace_id,
            "concurrent-usage-2",
            usage_request(&fixture, "concurrent-transaction-2", 600)
        )
    );
    assert!(first.is_ok() && second.is_ok());
    assert_usage_state(
        &fixture.pool,
        fixture.workspace_id,
        fixture.item_id,
        ExpectedUsageState::new(1_200, 1_000, 200, 1, 2),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_only_usage_skips_customer_ledger_and_insufficient_is_atomic() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let pending = setup_usage(1_000, 100, 500).await;
    let receipt = usage::record_usage(
        &pending.repository,
        pending.workspace_id,
        "pending-key",
        usage_request(&pending, "pending-transaction", 999),
    )
    .await
    .expect("pending usage");
    assert!(receipt.debit_id.is_none());
    assert_eq!(receipt.billing_status, "PENDING_BLOCK");
    assert_usage_state(
        &pending.pool,
        pending.workspace_id,
        pending.item_id,
        ExpectedUsageState::new(999, 0, 999, 0, 1),
    )
    .await;
    let transaction = get_json(
        &pending.router,
        &format!(
            "/v1/workspaces/{}/customer-wallet/transactions/pending-transaction",
            pending.workspace_id
        ),
    )
    .await;
    assert_eq!(transaction["transaction_id"], "pending-transaction");
    assert_eq!(transaction["pending_item_units_after"], "999");
    assert!(transaction.get("customer_wallet_entry_id").is_none());

    let insufficient = setup_usage(1, 7, 5).await;
    let result = usage::record_usage(
        &insufficient.repository,
        insufficient.workspace_id,
        "insufficient-key",
        usage_request(&insufficient, "insufficient-transaction", 1),
    )
    .await;
    assert_eq!(
        result.expect_err("insufficient credit").code(),
        "insufficient_credit"
    );
    assert_usage_state(
        &insufficient.pool,
        insufficient.workspace_id,
        insufficient.item_id,
        ExpectedUsageState::new(0, 0, 0, 0, 0),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_swagger_exposes_decimal_contract() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let fixture = setup_usage(1, 1, 1).await;
    let openapi = fixture
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("openapi");
    assert_eq!(openapi.status(), StatusCode::OK);
    let body = response_json(openapi).await;
    assert!(body["paths"]
        .get("/v1/workspaces/{workspace_id}/usage-events")
        .is_some());
    assert!(body["components"]["schemas"]
        .get("UsageEventResponse")
        .is_some());
    for schema in [
        "ProductEligibilityResponse",
        "ItemWalletMeterResponse",
        "ItemWalletStatementResponse",
        "BillingBlockResponse",
        "UsageReconciliationResponse",
        "WorkspaceTransactionResponse",
    ] {
        assert!(
            body["components"]["schemas"].get(schema).is_some(),
            "missing Swagger schema {schema}"
        );
    }
    for path in [
        "/v1/workspaces/{workspace_id}/products/{product_id}/eligibility",
        "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet",
        "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement",
        "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement/{entry_id}",
        "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/pricing-accumulators",
        "/v1/admin/workspaces/{workspace_id}/items/{item_id}/usage/reconcile",
    ] {
        assert!(
            body["paths"].get(path).is_some(),
            "missing Swagger path {path}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn item_meter_statement_eligibility_and_reconciliation_are_correlated() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let fixture = setup_usage(10, 3, 20).await;
    let receipt = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "query-key",
        usage_request(&fixture, "query-transaction", 14),
    )
    .await
    .expect("usage");
    let eligibility = get_json(
        &fixture.router,
        &format!(
            "/v1/workspaces/{}/products/{}/eligibility",
            fixture.workspace_id, fixture.product_id
        ),
    )
    .await;
    assert_eq!(eligibility["reason"], "eligible");
    assert_eq!(eligibility["balance_credit_units"], "17");

    let meter = get_json(
        &fixture.router,
        &format!(
            "/v1/workspaces/{}/items/{}/item-wallet",
            fixture.workspace_id, fixture.item_id
        ),
    )
    .await;
    assert_eq!(meter["pending_item_units"], "4");
    assert_eq!(meter["units_until_next_block"], "6");

    let statement = get_json(
        &fixture.router,
        &format!(
            "/v1/workspaces/{}/items/{}/item-wallet/statement?limit=1",
            fixture.workspace_id, fixture.item_id
        ),
    )
    .await;
    assert_eq!(statement["items"][0]["transaction_id"], "query-transaction");
    assert_eq!(
        statement["items"][0]["billing_block_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let entry = get_json(
        &fixture.router,
        &format!(
            "/v1/workspaces/{}/items/{}/item-wallet/statement/{}",
            fixture.workspace_id, fixture.item_id, receipt.item_wallet_entry_id
        ),
    )
    .await;
    assert_eq!(entry["usage_event_id"], receipt.usage_event_id.to_string());

    let reconciliation = post_json(
        &fixture.router,
        &format!(
            "/v1/admin/workspaces/{}/items/{}/usage/reconcile",
            fixture.workspace_id, fixture.item_id
        ),
    )
    .await;
    assert_eq!(reconciliation["consistent"], true);
    let references: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wallet_transaction_references WHERE customer_wallet_entry_id=$1",
    )
    .bind(receipt.customer_wallet_entry_id.expect("customer entry"))
    .fetch_one(&fixture.pool)
    .await
    .expect("references");
    assert_eq!(references, 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tier_boundary_assigns_unique_blocks_without_repricing() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let fixture = setup_tiered_usage(standard_tiers(), None, 100).await;
    let first = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "tier-key-1",
        usage_request(&fixture, "tier-transaction-1", 24),
    )
    .await
    .expect("tiered usage");
    assert_eq!(first.converted_item_units.value(), 20);
    assert_eq!(first.converted_blocks, 15);
    assert_eq!(first.debited_credit_units.value(), 15);
    assert_eq!(first.pending_item_units_after.value(), 4);
    let tier_counts: Vec<(Option<i32>, i64, i64)> = sqlx::query_as(
        "SELECT tier_position,count(*),sum(debited_credit_units)::bigint FROM billing_blocks \
         WHERE customer_id=$1 AND item_id=$2 GROUP BY tier_position ORDER BY tier_position",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .fetch_all(&fixture.pool)
    .await
    .expect("tier blocks");
    assert_eq!(tier_counts, vec![(Some(0), 10, 10), (Some(1), 5, 5)]);

    let second = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "tier-key-2",
        usage_request(&fixture, "tier-transaction-2", 1),
    )
    .await
    .expect("complete tier pending");
    assert_eq!(second.converted_item_units.value(), 5);
    assert_eq!(second.allocations[0].tier_position, Some(2));
    assert_eq!(second.debited_credit_units.value(), 2);
    let cycle_keys: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT cycle_key FROM pricing_accumulators WHERE customer_id=$1 AND item_id=$2",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .fetch_all(&fixture.pool)
    .await
    .expect("lifetime accumulator");
    assert_eq!(cycle_keys, vec!["lifetime"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cycle_boundary_uses_distinct_accumulators() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let anchor = chrono::Utc::now() - chrono::Duration::days(2) - chrono::Duration::hours(1);
    let rule = "FREQ=DAILY;INTERVAL=1";
    let fixture = setup_tiered_usage(
        standard_tiers(),
        Some(AccumulationCycleInput {
            anchor_at: anchor,
            recurrence_rule: rule.to_string(),
        }),
        100,
    )
    .await;
    let (current_start, _) = pricing_cycle_bounds(anchor, rule, chrono::Utc::now()).expect("cycle");
    let previous_start = current_start - chrono::Duration::days(1);
    let previous_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO pricing_accumulators (pricing_accumulator_id,customer_id,item_id,price_version_id, \
         cycle_key,accumulated_converted_item_units,converted_blocks) VALUES ($1,$2,$3,$4,$5,10,10)",
    )
    .bind(previous_id)
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .bind(fixture.price_id)
    .bind(previous_start.to_rfc3339())
    .execute(&fixture.pool)
    .await
    .expect("previous accumulator");
    let receipt = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "cycle-key",
        usage_request(&fixture, "cycle-transaction", 1),
    )
    .await
    .expect("current cycle usage");
    assert_eq!(receipt.allocations[0].cycle_key, current_start.to_rfc3339());
    let accumulators: Vec<(uuid::Uuid, i64)> = sqlx::query_as(
        "SELECT pricing_accumulator_id,accumulated_converted_item_units FROM pricing_accumulators \
         WHERE customer_id=$1 AND item_id=$2 ORDER BY accumulated_converted_item_units DESC",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .fetch_all(&fixture.pool)
    .await
    .expect("accumulators");
    assert_eq!(
        accumulators,
        vec![(previous_id, 10), (accumulators[1].0, 1)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_block_keeps_old_price_then_remainder_uses_active_version() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let fixture = setup_versioned_usage(100).await;
    let first = usage::record_usage(
        &fixture.usage.repository,
        fixture.usage.workspace_id,
        "version-key-1",
        usage_request(&fixture.usage, "version-transaction-1", 6),
    )
    .await
    .expect("old price pending");
    assert_eq!(first.pending_item_units_after.value(), 6);
    let delay = (fixture.boundary - chrono::Utc::now())
        .to_std()
        .unwrap_or_default()
        + std::time::Duration::from_millis(50);
    tokio::time::sleep(delay).await;

    let mut request = usage_request(&fixture.usage, "version-transaction-2", 8);
    request.expected_price_version_id = Some(fixture.new_price_id);
    let second = usage::record_usage(
        &fixture.usage.repository,
        fixture.usage.workspace_id,
        "version-key-2",
        request,
    )
    .await
    .expect("version transition usage");
    assert_eq!(second.converted_item_units.value(), 14);
    assert_eq!(second.debited_credit_units.value(), 11);
    assert_eq!(second.pending_item_units_after.value(), 0);
    assert_eq!(second.allocations.len(), 2);
    assert_eq!(second.allocations[0].price_version_id, fixture.old_price_id);
    assert_eq!(second.allocations[1].price_version_id, fixture.new_price_id);
    let blocks: Vec<(uuid::Uuid, i64, i64)> = sqlx::query_as(
        "SELECT price_version_id,unit_offset_start,unit_offset_end FROM billing_blocks \
         WHERE customer_id=$1 AND item_id=$2 ORDER BY global_block_sequence",
    )
    .bind(fixture.usage.workspace_id)
    .bind(fixture.usage.item_id)
    .fetch_all(&fixture.usage.pool)
    .await
    .expect("versioned blocks");
    assert_eq!(
        blocks,
        vec![
            (fixture.old_price_id, 0, 10),
            (fixture.new_price_id, 10, 14)
        ]
    );
}

async fn get_json(router: &axum::Router, uri: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn post_json(router: &axum::Router, uri: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn assert_usage_state(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    item_id: uuid::Uuid,
    expected: ExpectedUsageState,
) {
    let actual: (i64,i64,i64,i64,i64) = sqlx::query_as("SELECT iw.total_received_item_units,iw.total_converted_item_units,iw.pending_item_units,(SELECT count(*) FROM billing_blocks b WHERE b.customer_id=$1 AND b.item_id=$2),(SELECT count(*) FROM usage_events u WHERE u.customer_id=$1 AND u.item_id=$2) FROM item_wallets iw JOIN wallets w ON w.wallet_id=iw.wallet_id WHERE w.customer_id=$1 AND w.item_id=$2").bind(workspace_id).bind(item_id).fetch_one(pool).await.expect("usage state");
    assert_eq!(actual, expected.values);
}

struct ExpectedUsageState {
    values: (i64, i64, i64, i64, i64),
}

impl ExpectedUsageState {
    fn new(received: i64, converted: i64, pending: i64, blocks: i64, events: i64) -> Self {
        Self {
            values: (received, converted, pending, blocks, events),
        }
    }
}
