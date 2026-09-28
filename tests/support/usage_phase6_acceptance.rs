use super::*;
use subscription::{
    dto::catalog::{
        CatalogStatus, CreateItemRequest, CreatePriceVersionRequest, CreateProductRequest,
        PricingModel, UpdateItemRequest, UpdateProductRequest, UsageModel,
    },
    services::catalog,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_usage_consolidates_debit_and_partitions_every_received_unit() {
    let _guard = test_guard().await;
    let fixture = setup_usage(10, 2, 100).await;
    let receipt = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "multi-block-key",
        usage_request(&fixture, "multi-block-transaction", 25),
    )
    .await
    .unwrap();
    assert_eq!(receipt.converted_blocks, 2);
    assert_eq!(receipt.pending_item_units_after.value(), 5);
    assert_eq!(receipt.debited_credit_units.value(), 4);
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM usage_events WHERE customer_id=$1), \
         (SELECT count(*) FROM item_wallet_entries e JOIN wallets w ON w.wallet_id=e.item_wallet_id WHERE w.customer_id=$1), \
         (SELECT count(*) FROM debits d JOIN usage_events u ON u.usage_event_id=d.usage_event_id WHERE u.customer_id=$1), \
         (SELECT count(*) FROM customer_wallet_entries WHERE customer_id=$1 AND source_channel='usage')",
    )
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1, 1));
    let intervals: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT unit_offset_start,unit_offset_end FROM billing_blocks \
         WHERE customer_id=$1 AND item_id=$2 ORDER BY global_block_sequence",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(intervals, vec![(0, 10), (10, 20)]);
    assert!(
        usage::reconcile_item(&fixture.repository, fixture.workspace_id, fixture.item_id)
            .await
            .unwrap()
            .consistent
    );
    let wallet = get_json(
        &fixture.router,
        &format!("/v1/workspaces/{}/wallets", fixture.workspace_id),
    )
    .await;
    let latest_balance: i64 = sqlx::query_scalar(
        "SELECT balance_after_credit_units FROM customer_wallet_entries \
         WHERE customer_id=$1 ORDER BY entry_sequence DESC LIMIT 1",
    )
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(wallet["customer_wallet"]["balance_credit_units"], "96");
    assert_eq!(latest_balance, 96);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unit_prices_apply_declared_integer_ratios_through_the_maximum() {
    let _guard = test_guard().await;
    for (block, cost, credits, units, converted, pending, debit) in [
        (1, 1, 10, 3, 3, 0, 3),
        (10, 1, 10, 25, 20, 5, 2),
        (10, 10, 100, 25, 20, 5, 20),
        (1, i64::MAX, i64::MAX, 1, 1, 0, i64::MAX),
    ] {
        let fixture = setup_usage(block, cost, credits).await;
        let receipt = usage::record_usage(
            &fixture.repository,
            fixture.workspace_id,
            &format!("ratio-key-{block}-{cost}"),
            usage_request(
                &fixture,
                &format!("ratio-transaction-{block}-{cost}"),
                units,
            ),
        )
        .await
        .unwrap();
        assert_eq!(receipt.converted_item_units.value(), converted);
        assert_eq!(receipt.pending_item_units_after.value(), pending);
        assert_eq!(receipt.debited_credit_units.value(), debit);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_tier_crossing_keeps_unique_blocks_and_ordinals() {
    let _guard = test_guard().await;
    let fixture = setup_tiered_usage(standard_tiers(), None, 100).await;
    let first_repository = fixture.repository.clone();
    let second_repository = fixture.repository.clone();
    let (first, second) = tokio::join!(
        usage::record_usage(
            &first_repository,
            fixture.workspace_id,
            "tier-race-key-1",
            usage_request(&fixture, "tier-race-transaction-1", 12),
        ),
        usage::record_usage(
            &second_repository,
            fixture.workspace_id,
            "tier-race-key-2",
            usage_request(&fixture, "tier-race-transaction-2", 12),
        )
    );
    assert!(first.is_ok() && second.is_ok());
    let uniqueness: (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*),count(DISTINCT global_block_sequence), \
         count(DISTINCT (price_version_id,cycle_key,price_block_ordinal)) \
         FROM billing_blocks WHERE customer_id=$1 AND item_id=$2",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(uniqueness, (15, 15, 15));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn usage_validated_before_wallet_deactivation_completes_first() {
    let _guard = test_guard().await;
    let fixture = setup_usage(1, 1, 10).await;
    let mut balance_lock = fixture.pool.begin().await.unwrap();
    sqlx::query(
        "SELECT cw.wallet_id FROM customer_wallets cw JOIN wallets w USING(wallet_id) \
         WHERE w.customer_id=$1 FOR UPDATE OF cw",
    )
    .bind(fixture.workspace_id)
    .fetch_one(&mut *balance_lock)
    .await
    .unwrap();
    let repository = fixture.repository.clone();
    let workspace_id = fixture.workspace_id;
    let request = usage_request(&fixture, "validated-first-transaction", 1);
    let usage_task = tokio::spawn(async move {
        usage::record_usage(&repository, workspace_id, "validated-first-key", request).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let pool = fixture.pool.clone();
    let item_id = fixture.item_id;
    let disable_task = tokio::spawn(async move {
        sqlx::query(
            "UPDATE wallet_effective_states SET status='DISABLED' WHERE wallet_id=(SELECT wallet_id \
             FROM wallets WHERE customer_id=$1 AND item_id=$2)",
        )
        .bind(workspace_id)
        .bind(item_id)
        .execute(&pool)
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!usage_task.is_finished());
    assert!(!disable_task.is_finished());
    balance_lock.commit().await.unwrap();
    usage_task.await.unwrap().unwrap();
    disable_task.await.unwrap().unwrap();
    let state: (i64, String) = sqlx::query_as(
        "SELECT count(*),es.status FROM usage_events u JOIN wallets w ON w.customer_id=u.customer_id \
         AND w.item_id=u.item_id JOIN wallet_effective_states es ON es.wallet_id=w.wallet_id \
         WHERE u.customer_id=$1 GROUP BY es.status",
    )
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(state, (1, "DISABLED".into()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_http_enforces_positive_zero_and_insufficient_balance_boundaries() {
    let _guard = test_guard().await;
    for (cost, status, expected_balance) in [
        (3, axum::http::StatusCode::CREATED, Some("2")),
        (5, axum::http::StatusCode::CREATED, Some("0")),
        (7, axum::http::StatusCode::CONFLICT, None),
    ] {
        let fixture = setup_usage(1, cost, 5).await;
        let response = post_usage(&fixture, cost).await;
        assert_eq!(response.status(), status);
        let body = response_json(response).await;
        if let Some(balance) = expected_balance {
            assert_eq!(body["balance_after_credit_units"], balance);
        } else {
            assert_eq!(body["error"]["code"], "insufficient_credit");
            assert_atomic_counts(&fixture, 0, 0, 0).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eligibility_separates_decision_facts_and_never_authorizes_a_later_debit() {
    let _guard = test_guard().await;
    let fixture = setup_usage(10, 7, 5).await;
    assert_eligible_facts(&fixture, 5).await;
    usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "pending-facts-key",
        usage_request(&fixture, "pending-facts-transaction", 5),
    )
    .await
    .unwrap();
    assert_eligible_facts(&fixture, 5).await;
    let rejected = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "stale-decision-key",
        usage_request(&fixture, "stale-decision-transaction", 5),
    )
    .await
    .unwrap_err();
    assert_eq!(rejected.code(), "insufficient_credit");
    let state: (i64, i64, i64) = sqlx::query_as(
        "SELECT iw.total_received_item_units,iw.pending_item_units, \
         (SELECT count(*) FROM usage_events WHERE customer_id=$1) FROM item_wallets iw \
         JOIN wallets w ON w.wallet_id=iw.wallet_id WHERE w.customer_id=$1 AND w.item_id=$2",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(state, (5, 5, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn product_outside_customer_plan_is_rejected_without_usage_effects() {
    let _guard = test_guard().await;
    let fixture = setup_usage(1, 1, 10).await;
    let (product_id, item_id, price_id) = create_unentitled_catalog(&fixture.repository).await;
    fixture
        .repository
        .reconcile_wallets(fixture.workspace_id, Some("test:phase-6-scope"))
        .await
        .unwrap();
    let eligibility = usage::eligibility(&fixture.repository, fixture.workspace_id, product_id)
        .await
        .unwrap();
    assert!(!eligibility.access_allowed);
    assert!(!eligibility.customer_plan_entitled);
    assert!(eligibility.credit_sufficient);
    assert_eq!(eligibility.reason, "entitlement_not_granted");
    let mut request = usage_request(&fixture, "outside-plan-transaction", 1);
    request.product_id = product_id;
    request.item_id = item_id;
    request.expected_price_version_id = Some(price_id);
    let rejected = send_usage(&fixture, "outside-plan-key", request).await;
    assert_eq!(rejected.status(), axum::http::StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(rejected).await["error"]["code"],
        "product_not_entitled"
    );
    assert_atomic_counts(&fixture, 0, 0, 0).await;
}

async fn post_usage(fixture: &usage_fixture::UsageFixture, cost: i64) -> axum::response::Response {
    let request = usage_request(fixture, &format!("http-transaction-{cost}"), 1);
    send_usage(fixture, &format!("http-key-{cost}"), request).await
}

async fn send_usage(
    fixture: &usage_fixture::UsageFixture,
    key: &str,
    request: subscription::dto::usage::CreateUsageEventRequest,
) -> axum::response::Response {
    fixture
        .router
        .clone()
        .oneshot(
            Request::post(format!(
                "/v1/workspaces/{}/usage-events",
                fixture.workspace_id
            ))
            .header("content-type", "application/json")
            .header("idempotency-key", key)
            .body(Body::from(serde_json::to_vec(&request).unwrap()))
            .unwrap(),
        )
        .await
        .unwrap()
}

async fn assert_eligible_facts(fixture: &usage_fixture::UsageFixture, balance: i64) {
    let eligibility = usage::eligibility(
        &fixture.repository,
        fixture.workspace_id,
        fixture.product_id,
    )
    .await
    .unwrap();
    assert!(eligibility.access_allowed);
    assert!(eligibility.customer_plan_entitled);
    assert!(eligibility.credit_sufficient);
    assert_eq!(eligibility.balance_credit_units.unwrap().value(), balance);
    assert_eq!(eligibility.reason, "eligible");
}

async fn create_unentitled_catalog(
    repository: &subscription::repositories::database::DatabaseRepository,
) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid) {
    let product = catalog::create_product(
        repository,
        CreateProductRequest {
            name: format!("Outside plan {}", uuid::Uuid::new_v4()),
            description: None,
            usage_model: UsageModel::CreditMetered,
        },
    )
    .await
    .unwrap();
    let item = create_unentitled_item(repository, product.product_id).await;
    let price_id = create_unentitled_price(repository, item.item_id).await;
    activate_unentitled_catalog(repository, product.product_id, item.item_id).await;
    (product.product_id, item.item_id, price_id)
}

async fn create_unentitled_item(
    repository: &subscription::repositories::database::DatabaseRepository,
    product_id: uuid::Uuid,
) -> subscription::dto::catalog::ItemResponse {
    catalog::create_item(
        repository,
        product_id,
        CreateItemRequest {
            name: "Outside plan item".into(),
            parent_item_id: None,
            unit_name: Some("request".into()),
            quantity_scale: Some(ItemUnits::positive(1).unwrap()),
        },
    )
    .await
    .unwrap()
}

async fn create_unentitled_price(
    repository: &subscription::repositories::database::DatabaseRepository,
    item_id: uuid::Uuid,
) -> uuid::Uuid {
    let price = catalog::create_price_version(
        repository,
        item_id,
        CreatePriceVersionRequest {
            pricing_model: PricingModel::Unit,
            unit_block_size: Some(ItemUnits::positive(1).unwrap()),
            credit_units: Some(CreditUnits::new(1)),
            effective_from: chrono::Utc::now() - chrono::Duration::days(1),
            effective_until: None,
            accumulation_cycle: None,
            tiers: Vec::new(),
        },
    )
    .await
    .unwrap();
    catalog::publish_price_version(repository, price.price_version_id)
        .await
        .unwrap();
    price.price_version_id
}

async fn activate_unentitled_catalog(
    repository: &subscription::repositories::database::DatabaseRepository,
    product_id: uuid::Uuid,
    item_id: uuid::Uuid,
) {
    catalog::update_item(
        repository,
        item_id,
        UpdateItemRequest {
            name: None,
            parent_item_id: None,
            unit_name: None,
            quantity_scale: None,
            status: Some(CatalogStatus::Active),
            expected_version: 1,
        },
    )
    .await
    .unwrap();
    catalog::update_product(
        repository,
        product_id,
        UpdateProductRequest {
            name: None,
            description: None,
            usage_model: None,
            status: Some(CatalogStatus::Active),
            expected_version: 1,
        },
    )
    .await
    .unwrap();
}
