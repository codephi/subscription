mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use serde_json::Value;
use subscription::{
    dto::catalog::{CatalogStatus, UpdateItemRequest},
    services::{catalog, usage},
};
use usage_fixture::{setup_usage, usage_request, UsageFixture};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scope_removal_and_reactivation_preserve_identity_pending_and_statement() {
    let fixture = setup_usage(10, 1, 10).await;
    usage::record_usage(
        &fixture.repository,
        fixture.account_id,
        "pending-scope",
        usage_request(&fixture, "pending-scope", 6),
    )
    .await
    .unwrap();
    let before = scope_history(&fixture).await;
    assert_eq!(before.1[0]["pending_item_units"], 6);
    assert_eq!(before.2.as_array().unwrap().len(), 1);
    change_scope(&fixture, CatalogStatus::Inactive).await;
    assert_eq!(scope_history(&fixture).await, before);
    assert_scope_status(&fixture, "DISABLED").await;
    change_scope(&fixture, CatalogStatus::Active).await;
    assert_eq!(scope_history(&fixture).await, before);
    assert_scope_status(&fixture, "ACTIVE").await;
    let chain: Vec<(i64, Option<String>, String)> = sqlx::query_as(
        "SELECT e.sequence,e.previous_status,e.new_status FROM wallet_lifecycle_events e JOIN wallets w USING(wallet_id) WHERE w.item_id=$1 ORDER BY e.sequence",
    ).bind(fixture.item_id).fetch_all(&fixture.pool).await.unwrap();
    assert_eq!(
        chain,
        vec![
            (1, None, "PROVISIONING".into()),
            (2, Some("PROVISIONING".into()), "ACTIVE".into()),
            (3, Some("ACTIVE".into()), "DISABLED".into()),
            (4, Some("DISABLED".into()), "ACTIVE".into()),
        ]
    );
}

async fn change_scope(fixture: &UsageFixture, status: CatalogStatus) {
    let item = catalog::get_item(&fixture.repository, fixture.item_id)
        .await
        .unwrap();
    catalog::update_item(
        &fixture.repository,
        fixture.item_id,
        UpdateItemRequest {
            name: None,
            parent_item_id: None,
            unit_name: None,
            quantity_scale: None,
            status: Some(status),
            expected_version: item.version,
        },
    )
    .await
    .unwrap();
    fixture
        .repository
        .reconcile_wallets(fixture.account_id, Some("test:scope-change"))
        .await
        .unwrap();
}

async fn assert_scope_status(fixture: &UsageFixture, expected: &str) {
    let status: String = sqlx::query_scalar("SELECT s.status FROM wallet_effective_states s JOIN wallets w USING(wallet_id) WHERE w.item_id=$1")
        .bind(fixture.item_id).fetch_one(&fixture.pool).await.unwrap();
    assert_eq!(status, expected);
}

async fn scope_history(fixture: &UsageFixture) -> (Value, Value, Value) {
    sqlx::query_as("SELECT (SELECT jsonb_agg(to_jsonb(w) ORDER BY wallet_id) FROM wallets w), (SELECT jsonb_agg(to_jsonb(i) ORDER BY wallet_id) FROM item_wallets i), (SELECT jsonb_agg(to_jsonb(e) ORDER BY item_wallet_entry_id) FROM item_wallet_entries e)")
        .fetch_one(&fixture.pool).await.unwrap()
}
