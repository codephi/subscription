mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use sqlx::PgPool;
use std::time::Duration;
use subscription::{
    dto::catalog::{CatalogStatus, UpdateItemRequest},
    repositories::database::DatabaseRepository,
    services::catalog,
};
use usage_fixture::{setup_two_item_usage, MultiItemUsageFixture};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_catalog_scope_changes_preserve_every_committed_item() {
    let fixture = setup_two_item_usage(1, 10).await;
    let original = catalog::get_current_catalog_scope(&fixture.repository)
        .await
        .unwrap();
    change_items_concurrently(&fixture, CatalogStatus::Inactive).await;
    let empty = catalog::get_current_catalog_scope(&fixture.repository)
        .await
        .unwrap();
    assert!(
        empty.items.is_empty(),
        "both inactive items must leave the scope"
    );
    change_items_concurrently(&fixture, CatalogStatus::Active).await;
    let restored = catalog::get_current_catalog_scope(&fixture.repository)
        .await
        .unwrap();
    assert_eq!(restored.items.len(), 2);
    assert_eq!(restored.scope_version, original.scope_version);
    assert_eq!(restored.fingerprint, original.fingerprint);
}

async fn change_items_concurrently(fixture: &MultiItemUsageFixture, status: CatalogStatus) {
    let mut blocker = fixture.pool.begin().await.unwrap();
    sqlx::query("SELECT * FROM catalog_scope_current FOR UPDATE")
        .fetch_all(&mut *blocker)
        .await
        .unwrap();
    let workers = fixture.item_ids.map(|item| {
        let repository = fixture.repository.clone();
        tokio::spawn(async move { change_catalog_item(&repository, item, status).await })
    });
    wait_for_scope_writers(&fixture.pool).await;
    blocker.commit().await.unwrap();
    for worker in workers {
        worker.await.unwrap();
    }
}

async fn change_catalog_item(repository: &DatabaseRepository, item: Uuid, status: CatalogStatus) {
    let current = catalog::get_item(repository, item).await.unwrap();
    catalog::update_item(
        repository,
        item,
        UpdateItemRequest {
            name: None,
            status: Some(status),
            expected_version: current.version,
        },
    )
    .await
    .unwrap();
}

async fn wait_for_scope_writers(pool: &PgPool) {
    // PH-02: hold the pointer until both transactions reach scope serialization.
    // This also catches the old implementation, which waited only after its read.
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type='Lock' \
                 AND query LIKE '%catalog_scope_current%'",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("expected two concurrent catalog writers waiting on the scope");
}
