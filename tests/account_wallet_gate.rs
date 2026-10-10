mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use serde_json::{json, Value};
use subscription::{
    dto::events::AccountEventEnvelope,
    services::{account_events::process_account_event, credits, usage},
};
use usage_fixture::{apply_event, setup_usage, usage_request, UsageFixture};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delayed_account_states_preserve_wallets_and_reject_new_financial_effects() {
    let fixture = setup_usage(10, 1, 10).await;
    usage::record_usage(
        &fixture.repository,
        fixture.account_id,
        "initial",
        usage_request(&fixture, "initial", 6),
    )
    .await
    .unwrap();
    let initial = financial_snapshot(&fixture).await;
    let termination = delayed_termination(fixture.account_id);
    process_account_event(&fixture.repository, termination.clone())
        .await
        .unwrap();
    apply_event(
        &fixture.repository,
        fixture.account_id,
        "account.blocked",
        3,
    )
    .await;
    apply_event(
        &fixture.repository,
        fixture.account_id,
        "account.activated",
        2,
    )
    .await;
    assert_financial_operations_blocked(&fixture).await;
    assert_eq!(financial_snapshot(&fixture).await, initial);
    apply_event(
        &fixture.repository,
        fixture.account_id,
        "account.activated",
        4,
    )
    .await;
    fixture
        .repository
        .replay_account_event(termination.event_id)
        .await
        .unwrap();
    apply_event(
        &fixture.repository,
        fixture.account_id,
        "account.blocked",
        3,
    )
    .await;
    assert_financial_operations_blocked(&fixture).await;
    assert_eq!(financial_snapshot(&fixture).await, initial);
    let hierarchy = fixture
        .repository
        .find_wallet_hierarchy(fixture.account_id)
        .await
        .unwrap();
    assert!(!hierarchy.ready);
    assert_eq!(hierarchy.customer_wallet.status.as_str(), "DISABLED");
    assert!(hierarchy
        .item_wallets
        .iter()
        .all(|wallet| wallet.status.as_str() == "DISABLED"));
}

fn delayed_termination(account: Uuid) -> AccountEventEnvelope {
    serde_json::from_value(json!({
        "event_id":Uuid::new_v4(), "event_type":"account.terminated", "schema_version":1,
        "aggregate_id":account, "account_id":account, "sequence":5,
        "occurred_at":"2026-01-01T00:00:00Z", "correlation_id":Uuid::new_v4(),
        "payload":{"account_id":account}
    }))
    .unwrap()
}

async fn assert_financial_operations_blocked(fixture: &UsageFixture) {
    let eligibility_error =
        usage::eligibility(&fixture.repository, fixture.account_id, fixture.product_id)
            .await
            .unwrap_err();
    assert_eq!(eligibility_error.code(), "account_not_operational");
    let usage_error = usage::record_usage(
        &fixture.repository,
        fixture.account_id,
        "blocked-usage",
        usage_request(fixture, "blocked-usage", 5),
    )
    .await
    .unwrap_err();
    assert_eq!(usage_error.code(), "account_not_operational");
    let request =
        serde_json::from_value(json!({"transaction_id":"blocked-credit","credit_units":"20"}))
            .unwrap();
    let credit_error = credits::grant_direct_credit(
        &fixture.repository,
        fixture.account_id,
        "blocked-credit",
        request,
    )
    .await
    .unwrap_err();
    assert_eq!(credit_error.code(), "account_not_operational");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_provisioning_rejects_credit_and_eligibility_without_lazy_writes() {
    let fixture = setup_usage(10, 1, 10).await;
    sqlx::query("DELETE FROM wallet_provisioning WHERE customer_id=$1")
        .bind(fixture.account_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let before = financial_snapshot(&fixture).await;
    let request =
        serde_json::from_value(json!({"transaction_id":"missing-credit","credit_units":"20"}))
            .unwrap();
    let credit = credits::grant_direct_credit(
        &fixture.repository,
        fixture.account_id,
        "missing-credit",
        request,
    )
    .await
    .unwrap_err();
    assert_eq!(credit.code(), "wallet_not_provisioned");
    let eligibility =
        usage::eligibility(&fixture.repository, fixture.account_id, fixture.product_id)
            .await
            .unwrap_err();
    assert_eq!(eligibility.code(), "wallet_not_provisioned");
    assert_eq!(financial_snapshot(&fixture).await, before);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM wallet_provisioning")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

async fn financial_snapshot(fixture: &UsageFixture) -> Vec<Value> {
    let mut snapshot = Vec::new();
    for table in [
        "wallets",
        "customer_wallets",
        "item_wallets",
        "customer_wallet_entries",
        "item_wallet_entries",
        "credit_lots",
        "usage_events",
        "debits",
        "transaction_reservations",
        "idempotency_records",
        "customer_plans",
    ] {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT COALESCE(jsonb_agg(r ORDER BY r::text),'[]') FROM (SELECT to_jsonb(t) r FROM ",
        );
        query.push(table).push(" t) snapshot");
        snapshot.push(
            query
                .build_query_scalar()
                .fetch_one(&fixture.pool)
                .await
                .unwrap(),
        );
    }
    snapshot
}
