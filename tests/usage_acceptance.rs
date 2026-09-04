mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use std::sync::OnceLock;

use axum::{body::Body, http::Request};
use subscription::{
    dto::{
        catalog::{AccumulationCycleInput, PriceTierInput},
        units::{CreditUnits, ItemUnitBoundary, ItemUnits},
    },
    services::{calendar::pricing_cycle_bounds, usage},
};
use tokio::sync::Mutex;
use tower::ServiceExt;

use support::response_json;
use usage_fixture::{setup_tiered_usage, setup_usage, standard_tiers, usage_request};

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_strict_accepts_positive_and_zero_balance_then_rejects_negative() {
    let _guard = test_guard().await;
    let positive = setup_usage(1, 3, 5).await;
    let receipt = usage::record_usage(
        &positive.repository,
        positive.workspace_id,
        "positive-key",
        usage_request(&positive, "positive-transaction", 1),
    )
    .await
    .expect("positive balance");
    assert_eq!(receipt.balance_after_credit_units.unwrap().value(), 2);

    let zero = setup_usage(1, 5, 5).await;
    let receipt = usage::record_usage(
        &zero.repository,
        zero.workspace_id,
        "zero-key",
        usage_request(&zero, "zero-transaction", 1),
    )
    .await
    .expect("zero balance");
    assert_eq!(receipt.balance_after_credit_units.unwrap().value(), 0);
    let rejected = usage::record_usage(
        &zero.repository,
        zero.workspace_id,
        "negative-key",
        usage_request(&zero, "negative-transaction", 1),
    )
    .await
    .expect_err("negative balance");
    assert_eq!(rejected.code(), "insufficient_credit");
    assert_atomic_counts(&zero, 1, 1, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_overflow_and_insufficient_credit_roll_back_every_related_table() {
    let _guard = test_guard().await;
    let overflow = setup_usage(i64::MAX, 1, 1).await;
    usage::record_usage(
        &overflow.repository,
        overflow.workspace_id,
        "overflow-prime-key",
        usage_request(&overflow, "overflow-prime-transaction", i64::MAX - 1),
    )
    .await
    .expect("prime pending");
    let error = usage::record_usage(
        &overflow.repository,
        overflow.workspace_id,
        "overflow-key",
        usage_request(&overflow, "overflow-transaction", 2),
    )
    .await
    .expect_err("overflow");
    assert_eq!(error.code(), "usage_arithmetic_overflow");
    assert_atomic_counts(&overflow, 1, 0, 0).await;

    let insufficient = setup_usage(1, 7, 5).await;
    let error = usage::record_usage(
        &insufficient.repository,
        insufficient.workspace_id,
        "atomic-key",
        usage_request(&insufficient, "atomic-transaction", 1),
    )
    .await
    .expect_err("insufficient credit");
    assert_eq!(error.code(), "insufficient_credit");
    assert_atomic_counts(&insufficient, 0, 0, 0).await;
    let reservations: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM idempotency_records WHERE workspace_id=$1 AND operation_kind='USAGE') \
         +(SELECT count(*) FROM transaction_reservations WHERE workspace_id=$1 AND operation_kind='USAGE')",
    )
    .bind(insufficient.workspace_id)
    .fetch_one(&insufficient.pool)
    .await
    .expect("reservations");
    assert_eq!(reservations, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn price_and_catalog_conflicts_leave_usage_unchanged() {
    let _guard = test_guard().await;
    let fixture = setup_usage(10, 1, 10).await;
    let mut changed = usage_request(&fixture, "changed-price-transaction", 1);
    changed.expected_price_version_id = Some(uuid::Uuid::new_v4());
    let error = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "changed-price-key",
        changed,
    )
    .await
    .expect_err("changed price");
    assert_eq!(error.code(), "price_version_changed");
    let mut invalid_item = usage_request(&fixture, "invalid-item-transaction", 1);
    invalid_item.item_id = uuid::Uuid::new_v4();
    let error = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "invalid-item-key",
        invalid_item,
    )
    .await
    .expect_err("invalid item");
    assert_eq!(error.code(), "usage_catalog_not_found");
    assert_atomic_counts(&fixture, 0, 0, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn item_statement_cursor_is_stable_and_events_are_versioned() {
    let _guard = test_guard().await;
    let fixture = setup_usage(10, 1, 10).await;
    for index in 1..=3 {
        usage::record_usage(
            &fixture.repository,
            fixture.workspace_id,
            &format!("cursor-key-{index}"),
            usage_request(&fixture, &format!("cursor-transaction-{index}"), 1),
        )
        .await
        .expect("pending usage");
    }
    let first = get_json(
        &fixture.router,
        &format!(
            "/v1/workspaces/{}/items/{}/item-wallet/statement?limit=2",
            fixture.workspace_id, fixture.item_id
        ),
    )
    .await;
    let cursor = first["next_cursor"].as_str().expect("next cursor");
    usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "cursor-key-4",
        usage_request(&fixture, "cursor-transaction-4", 1),
    )
    .await
    .expect("concurrent new entry");
    let second = get_json(
        &fixture.router,
        &format!(
            "/v1/workspaces/{}/items/{}/item-wallet/statement?limit=2&cursor={cursor}",
            fixture.workspace_id, fixture.item_id
        ),
    )
    .await;
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["items"][0]["transaction_id"], "cursor-transaction-1");
    assert_eq!(
        second["items"][0]["product_id"],
        fixture.product_id.to_string()
    );
    let events: Vec<(String, i64, String)> = sqlx::query_as(
        "SELECT event_type,(payload->>'schema_version')::bigint,payload->>'event_type' \
         FROM outbox_events WHERE workspace_id=$1 AND event_type='usage.recorded' ORDER BY aggregate_sequence",
    )
    .bind(fixture.workspace_id)
    .fetch_all(&fixture.pool)
    .await
    .expect("usage events");
    assert_eq!(events.len(), 4);
    assert!(events
        .iter()
        .all(|event| event.0 == "usage.recorded" && event.1 == 1 && event.2 == "usage.recorded"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_history_is_append_only_and_reconciliation_detects_divergence() {
    let _guard = test_guard().await;
    let fixture = setup_usage(1, 1, 10).await;
    usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "history-key",
        usage_request(&fixture, "history-transaction", 1),
    )
    .await
    .expect("usage");
    for statement in [
        "UPDATE usage_events SET metadata='{}'::jsonb",
        "UPDATE debits SET debited_credit_units=2",
        "UPDATE item_wallet_entries SET metadata='{}'::jsonb",
        "UPDATE billing_blocks SET debited_credit_units=2",
        "UPDATE credit_lot_allocations SET allocated_credit_units=2",
    ] {
        assert!(sqlx::query(statement).execute(&fixture.pool).await.is_err());
    }
    sqlx::query(
        "UPDATE item_wallets SET total_received_item_units=total_received_item_units+1, \
         pending_item_units=pending_item_units+1,pending_price_version_id=$2, \
         pending_unit_block_size=2,pending_credit_units=1 WHERE wallet_id=$1",
    )
    .bind(
        sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT wallet_id FROM wallets WHERE customer_id=$1 AND item_id=$2",
        )
        .bind(fixture.workspace_id)
        .bind(fixture.item_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("item wallet"),
    )
    .bind(fixture.price_id)
    .execute(&fixture.pool)
    .await
    .expect("introduce meter divergence");
    let reconciliation =
        usage::reconcile_item(&fixture.repository, fixture.workspace_id, fixture.item_id)
            .await
            .expect("reconciliation");
    assert!(!reconciliation.consistent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn splitting_tiered_usage_does_not_change_blocks_or_total_debit() {
    let _guard = test_guard().await;
    let single = setup_tiered_usage(standard_tiers(), None, 100).await;
    let receipt = usage::record_usage(
        &single.repository,
        single.workspace_id,
        "single-tier-key",
        usage_request(&single, "single-tier-transaction", 25),
    )
    .await
    .expect("single tier call");
    assert_eq!(receipt.debited_credit_units.value(), 17);
    assert_eq!(receipt.converted_blocks, 16);

    let split = setup_tiered_usage(standard_tiers(), None, 100).await;
    for (index, units) in [7, 18].into_iter().enumerate() {
        usage::record_usage(
            &split.repository,
            split.workspace_id,
            &format!("split-tier-key-{index}"),
            usage_request(&split, &format!("split-tier-transaction-{index}"), units),
        )
        .await
        .expect("split tier call");
    }
    let totals: (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*),sum(debited_credit_units)::bigint, \
         count(DISTINCT (price_version_id,cycle_key,price_block_ordinal)) \
         FROM billing_blocks WHERE customer_id=$1 AND item_id=$2",
    )
    .bind(split.workspace_id)
    .bind(split.item_id)
    .fetch_one(&split.pool)
    .await
    .expect("split blocks");
    assert_eq!(totals, (16, 17, 16));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn renewal_inactive_plan_can_spend_existing_credits() {
    let _guard = test_guard().await;
    let fixture = setup_usage(1, 2, 5).await;
    sqlx::query(
        "UPDATE customer_plans SET commercial_status='PAST_DUE',renewal_status='RENEWAL_INACTIVE' \
         WHERE customer_plan_id=$1",
    )
    .bind(fixture.customer_plan_id)
    .execute(&fixture.pool)
    .await
    .expect("mark renewal inactive");
    let eligibility = usage::eligibility(
        &fixture.repository,
        fixture.workspace_id,
        fixture.product_id,
    )
    .await
    .expect("eligibility");
    assert!(eligibility.eligible);
    assert_eq!(eligibility.reason, "eligible");
    assert_eq!(
        eligibility.renewal_status.as_deref(),
        Some("RENEWAL_INACTIVE")
    );
    let receipt = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "renewal-inactive-key",
        usage_request(&fixture, "renewal-inactive-transaction", 1),
    )
    .await
    .expect("existing credit usage");
    assert_eq!(receipt.balance_after_credit_units.unwrap().value(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_block_completed_after_cycle_boundary_uses_new_cycle() {
    let _guard = test_guard().await;
    let anchor = chrono::Utc::now() - chrono::Duration::days(2) - chrono::Duration::hours(1);
    let rule = "FREQ=DAILY;INTERVAL=1";
    let tiers = vec![
        PriceTierInput {
            from_accumulated_units: ItemUnitBoundary::non_negative(0).unwrap(),
            to_accumulated_units: Some(ItemUnitBoundary::non_negative(10).unwrap()),
            unit_block_size: ItemUnits::positive(2).unwrap(),
            credit_units: CreditUnits::new(1),
        },
        PriceTierInput {
            from_accumulated_units: ItemUnitBoundary::non_negative(10).unwrap(),
            to_accumulated_units: None,
            unit_block_size: ItemUnits::positive(2).unwrap(),
            credit_units: CreditUnits::new(2),
        },
    ];
    let fixture = setup_tiered_usage(
        tiers,
        Some(AccumulationCycleInput {
            anchor_at: anchor,
            recurrence_rule: rule.to_string(),
        }),
        100,
    )
    .await;
    let (current_start, _) = pricing_cycle_bounds(anchor, rule, chrono::Utc::now()).unwrap();
    let previous_start = current_start - chrono::Duration::days(1);
    let wallet_id: uuid::Uuid =
        sqlx::query_scalar("SELECT wallet_id FROM wallets WHERE customer_id=$1 AND item_id=$2")
            .bind(fixture.workspace_id)
            .bind(fixture.item_id)
            .fetch_one(&fixture.pool)
            .await
            .expect("item wallet");
    sqlx::query(
        "UPDATE item_wallets SET total_received_item_units=11,total_converted_item_units=10, \
         total_converted_blocks=5,pending_item_units=1,pending_price_version_id=$2, \
         pending_tier_position=1,pending_unit_block_size=2,pending_credit_units=2 WHERE wallet_id=$1",
    )
    .bind(wallet_id)
    .bind(fixture.price_id)
    .execute(&fixture.pool)
    .await
    .expect("seed prior-cycle pending");
    sqlx::query(
        "INSERT INTO pricing_accumulators (pricing_accumulator_id,customer_id,item_id,price_version_id, \
         cycle_key,accumulated_converted_item_units,converted_blocks) VALUES ($1,$2,$3,$4,$5,10,5)",
    )
    .bind(uuid::Uuid::new_v4())
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
        "cross-cycle-key",
        usage_request(&fixture, "cross-cycle-transaction", 1),
    )
    .await
    .expect("complete pending in current cycle");
    assert_eq!(receipt.allocations[0].tier_position, Some(1));
    assert_eq!(receipt.allocations[0].cycle_key, current_start.to_rfc3339());
    let previous_units: i64 = sqlx::query_scalar(
        "SELECT accumulated_converted_item_units FROM pricing_accumulators \
         WHERE customer_id=$1 AND item_id=$2 AND cycle_key=$3",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .bind(previous_start.to_rfc3339())
    .fetch_one(&fixture.pool)
    .await
    .expect("previous accumulator remains");
    assert_eq!(previous_units, 10);
}

async fn test_guard() -> tokio::sync::MutexGuard<'static, ()> {
    TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await
}

async fn assert_atomic_counts(
    fixture: &usage_fixture::UsageFixture,
    events: i64,
    blocks: i64,
    accumulators: i64,
) {
    let actual: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM usage_events WHERE customer_id=$1), \
         (SELECT count(*) FROM billing_blocks WHERE customer_id=$1), \
         (SELECT count(*) FROM pricing_accumulators WHERE customer_id=$1)",
    )
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("atomic counts");
    assert_eq!(actual, (events, blocks, accumulators));
}

async fn get_json(router: &axum::Router, uri: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    assert!(response.status().is_success());
    response_json(response).await
}
