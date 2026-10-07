use super::{
    apply_account_event, create_billable_catalog, setup_router_with_options, DatabaseRepository,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wallet_shape_checks_reject_invalid_links_without_persisting_rows() {
    let (pool, account) = integrity_fixture().await;
    let (parent, item, scope): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT parent_customer_wallet_id,item_id,provisioning_scope_version FROM wallets WHERE customer_id=$1 AND wallet_type='ITEM'",
    ).bind(account).fetch_one(&pool).await.unwrap();
    let before = immutable_history(&pool).await;
    for (kind, owner, parent_id, item_id) in [
        ("CUSTOMER", Uuid::new_v4(), Some(parent), None),
        ("CUSTOMER", Uuid::new_v4(), None, Some(item)),
        ("CUSTOMER", Uuid::new_v4(), Some(parent), Some(item)),
        ("ITEM", account, None, Some(item)),
        ("ITEM", account, Some(parent), None),
        ("ITEM", account, None, None),
        ("ITEM", Uuid::new_v4(), Some(parent), Some(item)),
    ] {
        let error = sqlx::query("INSERT INTO wallets (wallet_id,customer_id,wallet_type,parent_customer_wallet_id,item_id,provisioning_scope_version) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(Uuid::new_v4()).bind(owner).bind(kind).bind(parent_id).bind(item_id).bind(scope)
            .execute(&pool).await.expect_err("invalid wallet shape or owner");
        assert_check_violation(error);
        assert_eq!(immutable_history(&pool).await, before);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_wallet_and_lifecycle_column_rejects_update_and_delete() {
    let (pool, _) = integrity_fixture().await;
    let before = immutable_history(&pool).await;
    for table in ["wallets", "wallet_lifecycle_events"] {
        let columns: Vec<String> = sqlx::query_scalar("SELECT column_name FROM information_schema.columns WHERE table_schema='public' AND table_name=$1 ORDER BY ordinal_position")
            .bind(table).fetch_all(&pool).await.unwrap();
        assert!(!columns.is_empty());
        for column in columns {
            // Names come only from the two fixed test tables, not from request input.
            let mut statement = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE ");
            statement
                .push(table)
                .push(" SET ")
                .push(&column)
                .push("=")
                .push(&column);
            assert_check_violation(statement.build().execute(&pool).await.unwrap_err());
        }
        let mut statement = sqlx::QueryBuilder::<sqlx::Postgres>::new("DELETE FROM ");
        statement.push(table);
        assert_check_violation(statement.build().execute(&pool).await.unwrap_err());
        assert_eq!(immutable_history(&pool).await, before);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_billable_scope_requires_reconciliation_and_materializes_once() {
    let (pool, account) = integrity_fixture().await;
    let repository = DatabaseRepository::new(pool.clone());
    let initial = repository.find_wallet_hierarchy(account).await.unwrap();
    create_billable_catalog(&repository, 1).await;
    let before = immutable_history(&pool).await;
    let error = repository.find_wallet_hierarchy(account).await.unwrap_err();
    assert_eq!(error.code(), "wallet_not_provisioned");
    assert_eq!(immutable_history(&pool).await, before);
    repository
        .reconcile_wallets(account, Some("test:scope"))
        .await
        .unwrap();
    let current = repository.find_wallet_hierarchy(account).await.unwrap();
    assert!(current.ready);
    assert_ne!(current.scope_version, initial.scope_version);
    assert_eq!(current.item_wallets.len(), 2);
    let after = immutable_history(&pool).await;
    repository
        .reconcile_wallets(account, Some("test:retry"))
        .await
        .unwrap();
    assert_eq!(immutable_history(&pool).await, after);
}

async fn integrity_fixture() -> (PgPool, Uuid) {
    let (_, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    create_billable_catalog(&repository, 1).await;
    let account = Uuid::new_v4();
    apply_account_event(&repository, account, "account.created", 1).await;
    apply_account_event(&repository, account, "account.activated", 2).await;
    (pool, account)
}

fn assert_check_violation(error: sqlx::Error) {
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
}

async fn immutable_history(pool: &PgPool) -> (Value, Value) {
    sqlx::query_as("SELECT (SELECT jsonb_agg(to_jsonb(w) ORDER BY wallet_id) FROM wallets w), (SELECT jsonb_agg(to_jsonb(e) ORDER BY wallet_id,sequence) FROM wallet_lifecycle_events e)")
        .fetch_one(pool).await.unwrap()
}
