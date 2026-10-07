mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use axum::response::IntoResponse;
use serde_json::Value;
use subscription::services::usage;
use usage_fixture::{setup_usage, usage_request};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_lots_reject_usage_with_conflict_and_no_persisted_effects() {
    let fixture = setup_usage(1, 7, 10).await;
    sqlx::query("UPDATE credit_lots SET expires_at=now()-interval '1 second' WHERE customer_id=$1")
        .bind(fixture.account_id)
        .execute(&fixture.pool)
        .await
        .expect("expired lot before worker");
    let before = usage_snapshot(&fixture).await;
    let error = usage::record_usage(
        &fixture.repository,
        fixture.account_id,
        "expired-lot-key",
        usage_request(&fixture, "expired-lot-transaction", 1),
    )
    .await
    .expect_err("expired credits cannot cover debit");
    assert_eq!(error.code(), "insufficient_credit");
    assert_eq!(error.into_response().status(), 409);
    assert_eq!(usage_snapshot(&fixture).await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_event_payload_matches_persisted_correlation() {
    let fixture = setup_usage(1, 1, 10).await;
    let mismatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE account_id=$1 AND aggregate_type='customer_plan' \
         AND (payload->>'correlation_id') IS DISTINCT FROM correlation_id::text")
        .bind(fixture.account_id).fetch_one(&fixture.pool).await.expect("event correlations");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE account_id=$1 AND aggregate_type='customer_plan'",
    )
    .bind(fixture.account_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("plan events exist");
    assert!(count > 0);
    assert_eq!(mismatches, 0);
}

async fn usage_snapshot(fixture: &usage_fixture::UsageFixture) -> Vec<Value> {
    let mut snapshot = Vec::new();
    for table in [
        "usage_events",
        "item_wallet_entries",
        "pricing_accumulators",
        "billing_blocks",
        "debits",
        "customer_wallet_entries",
        "wallet_transaction_references",
        "credit_lots",
        "credit_lot_allocations",
        "customer_wallets",
        "item_wallets",
        "idempotency_records",
        "transaction_reservations",
        "outbox_events",
    ] {
        // The fixture owns its database: snapshot complete rows, including typed references,
        // to detect partial writes even when a rejected usage has no surviving parent row.
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT COALESCE(jsonb_agg(r ORDER BY r::text), '[]') FROM (SELECT to_jsonb(t) r FROM ",
        );
        query.push(table).push(" t) rows");
        snapshot.push(
            query
                .build_query_scalar()
                .fetch_one(&fixture.pool)
                .await
                .expect("account snapshot"),
        );
    }
    snapshot
}
