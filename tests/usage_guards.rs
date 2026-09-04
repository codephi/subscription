mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use std::sync::OnceLock;

use subscription::{
    dto::{
        catalog::{
            CatalogStatus, CreateItemRequest, CreatePriceVersionRequest, PricingModel,
            UpdateItemRequest,
        },
        credits::DirectCreditRequest,
        plans::RevokeCustomerPlanRequest,
        units::{CreditUnits, ItemUnits},
    },
    services::{catalog, credits, plans, usage},
};

use usage_fixture::{apply_event, setup_two_item_usage, setup_usage, usage_request};

static TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_item_usage_is_serialized_and_keys_have_one_effect() {
    let _guard = TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let fixture = setup_usage(1_000, 100, 500).await;
    let first_repository = fixture.repository.clone();
    let second_repository = fixture.repository.clone();
    let (first, second) = tokio::join!(
        usage::record_usage(
            &first_repository,
            fixture.workspace_id,
            "same-item-key-1",
            usage_request(&fixture, "same-item-transaction-1", 600),
        ),
        usage::record_usage(
            &second_repository,
            fixture.workspace_id,
            "same-item-key-2",
            usage_request(&fixture, "same-item-transaction-2", 600),
        )
    );
    assert!(first.is_ok() && second.is_ok());
    let ordered: Vec<(i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT total_received_item_units_after,accepted_at FROM item_wallet_entries \
         WHERE item_wallet_id=$1 ORDER BY total_received_item_units_after",
    )
    .bind(first.expect("first").item_wallet_id)
    .fetch_all(&fixture.pool)
    .await
    .expect("ordered usage");
    assert!(ordered[0].1 <= ordered[1].1);

    let duplicate = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "same-item-key-3",
        usage_request(&fixture, "same-item-transaction-1", 10),
    )
    .await
    .expect_err("duplicate transaction");
    assert_eq!(duplicate.code(), "transaction_already_exists");
    let reused_key = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "same-item-key-1",
        usage_request(&fixture, "different-transaction", 10),
    )
    .await
    .expect_err("duplicate key");
    assert_eq!(reused_key.code(), "idempotency_key_already_used");
    assert_usage_counts(&fixture.pool, fixture.workspace_id, 2, 1_200).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_usage_deduplication_persists_exactly_one_effect() {
    let _guard = TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let transaction_fixture = setup_usage(1, 1, 10).await;
    let first_repository = transaction_fixture.repository.clone();
    let second_repository = transaction_fixture.repository.clone();
    let (first, second) = tokio::join!(
        usage::record_usage(
            &first_repository,
            transaction_fixture.workspace_id,
            "transaction-race-key-1",
            usage_request(&transaction_fixture, "shared-usage-transaction", 1),
        ),
        usage::record_usage(
            &second_repository,
            transaction_fixture.workspace_id,
            "transaction-race-key-2",
            usage_request(&transaction_fixture, "shared-usage-transaction", 1),
        )
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(
        first
            .err()
            .or_else(|| second.err())
            .expect("one conflict")
            .code(),
        "transaction_already_exists"
    );
    assert_usage_counts(
        &transaction_fixture.pool,
        transaction_fixture.workspace_id,
        1,
        1,
    )
    .await;

    let key_fixture = setup_usage(10, 1, 10).await;
    let first_repository = key_fixture.repository.clone();
    let second_repository = key_fixture.repository.clone();
    let (first, second) = tokio::join!(
        usage::record_usage(
            &first_repository,
            key_fixture.workspace_id,
            "shared-usage-key",
            usage_request(&key_fixture, "key-race-transaction-1", 1),
        ),
        usage::record_usage(
            &second_repository,
            key_fixture.workspace_id,
            "shared-usage-key",
            usage_request(&key_fixture, "key-race-transaction-2", 2),
        )
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(
        first
            .err()
            .or_else(|| second.err())
            .expect("one conflict")
            .code(),
        "idempotency_key_already_used"
    );
    let received: i64 =
        sqlx::query_scalar("SELECT sum(item_units)::bigint FROM usage_events WHERE customer_id=$1")
            .bind(key_fixture.workspace_id)
            .fetch_one(&key_fixture.pool)
            .await
            .expect("received units");
    assert!(received == 1 || received == 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_plan_entitlement_and_missing_wallet_reject_atomically() {
    let _guard = TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let blocked = setup_usage(1, 1, 10).await;
    apply_event(
        &blocked.repository,
        blocked.workspace_id,
        "workspace.blocked",
        3,
    )
    .await;
    let error = usage::record_usage(
        &blocked.repository,
        blocked.workspace_id,
        "blocked-key",
        usage_request(&blocked, "blocked-transaction", 1),
    )
    .await
    .expect_err("blocked workspace");
    assert_eq!(error.code(), "workspace_not_operational");
    assert_usage_counts(&blocked.pool, blocked.workspace_id, 0, 0).await;

    let revoked = setup_usage(1, 1, 10).await;
    plans::revoke_customer_plan(
        &revoked.repository,
        revoked.workspace_id,
        revoked.customer_plan_id,
        RevokeCustomerPlanRequest {
            reason: "policy".to_string(),
            actor_reference: "operator:test".to_string(),
        },
    )
    .await
    .expect("revoke plan");
    let error = usage::record_usage(
        &revoked.repository,
        revoked.workspace_id,
        "revoked-key",
        usage_request(&revoked, "revoked-transaction", 1),
    )
    .await
    .expect_err("inactive plan");
    assert_eq!(error.code(), "customer_plan_not_active");
    assert_usage_counts(&revoked.pool, revoked.workspace_id, 0, 0).await;

    let unentitled = setup_usage(1, 1, 10).await;
    sqlx::query(
        "UPDATE customer_plan_entitlements SET effective_until=effective_from+interval '1 microsecond' \
         WHERE customer_plan_id=$1 AND effective_until IS NULL",
    )
    .bind(unentitled.customer_plan_id)
    .execute(&unentitled.pool)
    .await
    .expect("expire entitlement");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let error = usage::record_usage(
        &unentitled.repository,
        unentitled.workspace_id,
        "unentitled-key",
        usage_request(&unentitled, "unentitled-transaction", 1),
    )
    .await
    .expect_err("missing entitlement");
    assert_eq!(error.code(), "entitlement_not_granted");
    assert_usage_counts(&unentitled.pool, unentitled.workspace_id, 0, 0).await;

    let absent = setup_usage(1, 1, 10).await;
    let new_item = create_active_item_without_wallet(&absent).await;
    let mut request = usage_request(&absent, "missing-wallet-transaction", 1);
    request.item_id = new_item.0;
    request.expected_price_version_id = Some(new_item.1);
    let error = usage::record_usage(
        &absent.repository,
        absent.workspace_id,
        "missing-wallet-key",
        request,
    )
    .await
    .expect_err("missing wallet");
    assert_eq!(error.code(), "wallet_not_provisioned");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wallets WHERE customer_id=$1 AND item_id=$2")
            .bind(absent.workspace_id)
            .bind(new_item.0)
            .fetch_one(&absent.pool)
            .await
            .expect("wallet count");
    assert_eq!(count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_lot_allocation_prefers_subscription_then_persistent_credit() {
    let _guard = TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let fixture = setup_usage(1, 60, 50).await;
    credits::grant_direct_credit(
        &fixture.repository,
        fixture.workspace_id,
        "allocation-credit-key",
        DirectCreditRequest {
            transaction_id: "allocation-credit-transaction".to_string(),
            credit_units: CreditUnits::new(50),
            external_reference: None,
            description: None,
            metadata: None,
        },
    )
    .await
    .expect("direct credit");
    let receipt = usage::record_usage(
        &fixture.repository,
        fixture.workspace_id,
        "allocation-usage-key",
        usage_request(&fixture, "allocation-usage-transaction", 1),
    )
    .await
    .expect("usage debit");
    let allocations: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT l.source_kind,a.allocated_credit_units,a.allocation_ordinal \
         FROM credit_lot_allocations a JOIN credit_lots l ON l.credit_lot_id=a.credit_lot_id \
         WHERE a.debit_id=$1 ORDER BY a.allocation_ordinal",
    )
    .bind(receipt.debit_id.expect("debit"))
    .fetch_all(&fixture.pool)
    .await
    .expect("lot allocations");
    assert_eq!(
        allocations,
        vec![
            ("SUBSCRIPTION".to_string(), 50, 1),
            ("DIRECT".to_string(), 10, 2)
        ]
    );
    let mutation =
        sqlx::query("UPDATE credit_lot_allocations SET allocated_credit_units=1 WHERE debit_id=$1")
            .bind(receipt.debit_id.expect("debit"))
            .execute(&fixture.pool)
            .await;
    assert!(mutation.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn different_items_share_customer_balance_safely() {
    let _guard = TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let fixture = setup_two_item_usage(60, 100).await;
    let request = |index: usize| subscription::dto::usage::CreateUsageEventRequest {
        transaction_id: format!("different-item-transaction-{index}"),
        product_id: fixture.product_id,
        item_id: fixture.item_ids[index],
        item_units: ItemUnits::positive(1).expect("unit"),
        expected_price_version_id: Some(fixture.price_ids[index]),
        occurred_at: None,
        metadata: None,
    };
    let first_repository = fixture.repository.clone();
    let second_repository = fixture.repository.clone();
    let (first, second) = tokio::join!(
        usage::record_usage(
            &first_repository,
            fixture.workspace_id,
            "different-item-key-0",
            request(0),
        ),
        usage::record_usage(
            &second_repository,
            fixture.workspace_id,
            "different-item-key-1",
            request(1),
        )
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(
        first
            .err()
            .or_else(|| second.err())
            .expect("one rejection")
            .code(),
        "insufficient_credit"
    );
    let actual: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM usage_events WHERE customer_id=$1), \
         (SELECT cw.balance_credit_units FROM customer_wallets cw JOIN wallets w \
          ON w.wallet_id=cw.wallet_id WHERE w.customer_id=$1)",
    )
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("shared balance");
    assert_eq!(actual, (1, 40));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wallet_deactivation_serializes_with_usage() {
    let _guard = TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let fixture = setup_usage(1, 1, 10).await;
    let mut transaction = fixture.pool.begin().await.expect("transaction");
    sqlx::query(
        "SELECT iw.wallet_id FROM item_wallets iw JOIN wallets w ON w.wallet_id=iw.wallet_id \
         JOIN wallet_effective_states es ON es.wallet_id=iw.wallet_id \
         WHERE w.customer_id=$1 AND w.item_id=$2 FOR UPDATE OF iw,es",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("lock wallet");
    let repository = fixture.repository.clone();
    let workspace_id = fixture.workspace_id;
    let request = usage_request(&fixture, "deactivation-transaction", 1);
    let operation = tokio::spawn(async move {
        usage::record_usage(&repository, workspace_id, "deactivation-key", request).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    sqlx::query(
        "UPDATE wallet_effective_states SET status='DISABLED' WHERE wallet_id=(SELECT wallet_id \
         FROM wallets WHERE customer_id=$1 AND item_id=$2)",
    )
    .bind(fixture.workspace_id)
    .bind(fixture.item_id)
    .execute(&mut *transaction)
    .await
    .expect("disable wallet");
    transaction.commit().await.expect("commit disable");
    let error = operation
        .await
        .expect("usage task")
        .expect_err("disabled wallet");
    assert_eq!(error.code(), "item_wallet_not_active");
    assert_usage_counts(&fixture.pool, fixture.workspace_id, 0, 0).await;
}

async fn create_active_item_without_wallet(
    fixture: &usage_fixture::UsageFixture,
) -> (uuid::Uuid, uuid::Uuid) {
    let item = catalog::create_item(
        &fixture.repository,
        fixture.product_id,
        CreateItemRequest {
            name: format!("Late item {}", uuid::Uuid::new_v4()),
            parent_item_id: None,
            unit_name: Some("request".to_string()),
            quantity_scale: Some(ItemUnits::positive(1).expect("scale")),
        },
    )
    .await
    .expect("item");
    let price = catalog::create_price_version(
        &fixture.repository,
        item.item_id,
        CreatePriceVersionRequest {
            pricing_model: PricingModel::Unit,
            unit_block_size: Some(ItemUnits::positive(1).expect("block")),
            credit_units: Some(CreditUnits::new(1)),
            effective_from: chrono::Utc::now() - chrono::Duration::days(1),
            effective_until: None,
            accumulation_cycle: None,
            tiers: Vec::new(),
        },
    )
    .await
    .expect("price");
    catalog::publish_price_version(&fixture.repository, price.price_version_id)
        .await
        .expect("publish");
    catalog::update_item(
        &fixture.repository,
        item.item_id,
        UpdateItemRequest {
            name: None,
            status: Some(CatalogStatus::Active),
            expected_version: 1,
        },
    )
    .await
    .expect("activate");
    (item.item_id, price.price_version_id)
}

async fn assert_usage_counts(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    events: i64,
    received: i64,
) {
    let actual: (i64, i64) = sqlx::query_as(
        "SELECT count(*),COALESCE(sum(item_units),0)::bigint FROM usage_events WHERE customer_id=$1",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .expect("usage counts");
    assert_eq!(actual, (events, received));
}
