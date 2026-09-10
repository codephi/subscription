mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use serde_json::{json, Value};
use subscription::{
    dto::{
        catalog::{CatalogStatus, UpdateItemRequest},
        credits::DirectCreditRequest,
        units::ItemUnits,
        usage::CreateUsageEventRequest,
        wallets::WalletStatus,
    },
    services::{catalog, credits, usage},
};
use usage_fixture::{setup_two_item_usage, MultiItemUsageFixture};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn returning_scope_requires_active_hierarchy_before_any_new_effect() {
    let fixture = setup_two_item_usage(1, 10).await;
    let original = fixture
        .repository
        .find_wallet_hierarchy(fixture.workspace_id)
        .await
        .unwrap();
    change_first_item(&fixture, CatalogStatus::Inactive).await;
    reconcile_scope(&fixture).await;
    change_first_item(&fixture, CatalogStatus::Active).await;
    assert_unready_scope(&fixture).await;
    assert_incomplete_scope_rejects_effects(&fixture).await;
    reconcile_scope(&fixture).await;
    assert_recovered_scope(&fixture, original.scope_version).await;
    let restored = fixture
        .repository
        .find_wallet_hierarchy(fixture.workspace_id)
        .await
        .unwrap();
    assert_eq!(
        restored.customer_wallet.wallet_id,
        original.customer_wallet.wallet_id
    );
    assert_eq!(
        restored
            .item_wallets
            .iter()
            .map(|w| w.wallet_id)
            .collect::<Vec<_>>(),
        original
            .item_wallets
            .iter()
            .map(|w| w.wallet_id)
            .collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn historical_provisioning_counts_do_not_hide_missing_wallet_components() {
    let fixture = setup_two_item_usage(1, 10).await;
    for table in ["wallet_effective_states", "item_wallets"] {
        let mut statement = sqlx::QueryBuilder::<sqlx::Postgres>::new("DELETE FROM ");
        statement
            .push(table)
            .push(" WHERE wallet_id=(SELECT wallet_id FROM wallets WHERE customer_id=")
            .push_bind(fixture.workspace_id)
            .push(" AND item_id=")
            .push_bind(fixture.item_ids[0])
            .push(")");
        statement.build().execute(&fixture.pool).await.unwrap();
        assert_unready_scope(&fixture).await;
        assert_incomplete_scope_rejects_effects(&fixture).await;
        reconcile_scope(&fixture).await;
        let restored = fixture
            .repository
            .find_wallet_hierarchy(fixture.workspace_id)
            .await
            .unwrap();
        assert!(restored.ready);
        assert_eq!(restored.item_wallets.len(), 2);
    }
}

async fn change_first_item(fixture: &MultiItemUsageFixture, status: CatalogStatus) {
    let item = catalog::get_item(&fixture.repository, fixture.item_ids[0])
        .await
        .unwrap();
    catalog::update_item(
        &fixture.repository,
        item.item_id,
        UpdateItemRequest {
            name: None,
            status: Some(status),
            expected_version: item.version,
        },
    )
    .await
    .unwrap();
}

async fn reconcile_scope(fixture: &MultiItemUsageFixture) {
    let result = fixture
        .repository
        .reconcile_wallets(fixture.workspace_id, Some("test:readiness"))
        .await
        .unwrap();
    assert_eq!(result.status, WalletStatus::Active);
}

async fn assert_unready_scope(fixture: &MultiItemUsageFixture) {
    let hierarchy = fixture
        .repository
        .find_wallet_hierarchy(fixture.workspace_id)
        .await
        .unwrap();
    assert!(!hierarchy.ready);
    let provisioning = fixture
        .repository
        .find_wallet_provisioning(fixture.workspace_id)
        .await
        .unwrap();
    assert_eq!(provisioning.status, WalletStatus::Provisioning);
    assert!(provisioning.completed_at.is_none());
}

async fn assert_incomplete_scope_rejects_effects(fixture: &MultiItemUsageFixture) {
    let before = readiness_financial_snapshot(fixture).await;
    let credit = credits::grant_direct_credit(
        &fixture.repository,
        fixture.workspace_id,
        "readiness-credit",
        readiness_credit_request(),
    )
    .await
    .unwrap_err();
    assert_eq!(credit.code(), "wallet_not_provisioned");
    let eligibility = usage::eligibility(
        &fixture.repository,
        fixture.workspace_id,
        fixture.product_id,
    )
    .await
    .unwrap_err();
    assert_eq!(eligibility.code(), "wallet_not_provisioned");
    let consumption = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "readiness-usage",
        readiness_usage_request(fixture),
    )
    .await
    .unwrap_err();
    assert_eq!(consumption.code(), "wallet_not_provisioned");
    assert_eq!(readiness_financial_snapshot(fixture).await, before);
}

async fn assert_recovered_scope(fixture: &MultiItemUsageFixture, scope: uuid::Uuid) {
    let hierarchy = fixture
        .repository
        .find_wallet_hierarchy(fixture.workspace_id)
        .await
        .unwrap();
    assert!(hierarchy.ready);
    assert_eq!(hierarchy.scope_version, scope);
    assert!(hierarchy
        .item_wallets
        .iter()
        .all(|w| w.status == WalletStatus::Active));
    let before_retry = readiness_financial_snapshot(fixture).await;
    reconcile_scope(fixture).await;
    assert_eq!(readiness_financial_snapshot(fixture).await, before_retry);
    let credit = credits::grant_direct_credit(
        &fixture.repository,
        fixture.workspace_id,
        "readiness-credit",
        readiness_credit_request(),
    )
    .await
    .unwrap();
    assert_eq!(credit.entry.balance_after_credit_units.value(), 11);
    usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "readiness-usage",
        readiness_usage_request(fixture),
    )
    .await
    .unwrap();
}

fn readiness_credit_request() -> DirectCreditRequest {
    serde_json::from_value(json!({"transaction_id":"readiness-credit","credit_units":"1"})).unwrap()
}

fn readiness_usage_request(fixture: &MultiItemUsageFixture) -> CreateUsageEventRequest {
    CreateUsageEventRequest {
        transaction_id: "readiness-usage".into(),
        product_id: fixture.product_id,
        item_id: fixture.item_ids[1],
        item_units: ItemUnits::positive(1).unwrap(),
        expected_price_version_id: Some(fixture.price_ids[1]),
        occurred_at: None,
        metadata: None,
    }
}

async fn readiness_financial_snapshot(fixture: &MultiItemUsageFixture) -> Vec<Value> {
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
        "wallet_lifecycle_events",
        "outbox_events",
    ] {
        let mut statement = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT COALESCE(jsonb_agg(r ORDER BY r::text),'[]') FROM (SELECT to_jsonb(t) r FROM ",
        );
        statement.push(table).push(" t) snapshot");
        snapshot.push(
            statement
                .build_query_scalar()
                .fetch_one(&fixture.pool)
                .await
                .unwrap(),
        );
    }
    snapshot
}
