use super::{
    apply_workspace_event, create_billable_catalog, setup_router_with_options, DatabaseRepository,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconciliation_rebuilds_missing_and_corrupt_projections_without_rewriting_history() {
    let (pool, workspace) = recovery_fixture().await;
    let repository = DatabaseRepository::new(pool.clone());
    let before = lifecycle_history(&pool).await;
    for statement in [
        "DELETE FROM wallet_effective_states",
        "UPDATE wallet_effective_states SET status='ERROR',lifecycle_sequence=99",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
        repository
            .reconcile_wallets(workspace, Some("test:restore"))
            .await
            .unwrap();
        assert_eq!(lifecycle_history(&pool).await, before);
        assert_projection_matches_history(&pool).await;
        assert!(
            repository
                .find_wallet_hierarchy(workspace)
                .await
                .unwrap()
                .ready
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconciliation_preserves_durable_error_and_appends_recovery_from_history() {
    let (pool, workspace) = recovery_fixture().await;
    let wallet: Uuid =
        sqlx::query_scalar("SELECT wallet_id FROM wallets WHERE wallet_type='CUSTOMER'")
            .fetch_one(&pool)
            .await
            .unwrap();
    // A restored database can contain durable history without its disposable projection.
    sqlx::query("INSERT INTO wallet_lifecycle_events (wallet_lifecycle_event_id,wallet_id,sequence,previous_status,new_status,reason,actor_reference,correlation_id) VALUES ($1,$2,3,'ACTIVE','ERROR','restored provisioning failure','test:restore',$3)")
        .bind(Uuid::new_v4()).bind(wallet).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    let before = lifecycle_history(&pool).await;
    sqlx::query("DELETE FROM wallet_effective_states")
        .execute(&pool)
        .await
        .unwrap();
    let repository = DatabaseRepository::new(pool.clone());
    repository
        .reconcile_wallets(workspace, Some("test:recovery"))
        .await
        .unwrap();
    let recovery: (i64, String, String, String) = sqlx::query_as("SELECT sequence,previous_status,new_status,actor_reference FROM wallet_lifecycle_events WHERE wallet_id=$1 ORDER BY sequence DESC LIMIT 1")
        .bind(wallet).fetch_one(&pool).await.unwrap();
    assert_eq!(
        recovery,
        (4, "ERROR".into(), "ACTIVE".into(), "test:recovery".into())
    );
    let after = lifecycle_history(&pool).await;
    for original in before.as_array().unwrap() {
        assert!(after.as_array().unwrap().contains(original));
    }
    assert_projection_matches_history(&pool).await;
    repository
        .reconcile_wallets(workspace, Some("test:retry"))
        .await
        .unwrap();
    assert_eq!(lifecycle_history(&pool).await, after);
}

async fn recovery_fixture() -> (PgPool, Uuid) {
    let (_, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    create_billable_catalog(&repository, 1).await;
    let workspace = Uuid::new_v4();
    apply_workspace_event(&repository, workspace, "workspace.created", 1).await;
    apply_workspace_event(&repository, workspace, "workspace.activated", 2).await;
    (pool, workspace)
}

async fn lifecycle_history(pool: &PgPool) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(e) ORDER BY wallet_id,sequence) FROM wallet_lifecycle_events e",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_projection_matches_history(pool: &PgPool) {
    let mismatches: i64 = sqlx::query_scalar("SELECT count(*) FROM (SELECT DISTINCT ON(wallet_id) wallet_id,new_status,sequence FROM wallet_lifecycle_events ORDER BY wallet_id,sequence DESC) e FULL JOIN wallet_effective_states s USING(wallet_id) WHERE s.status IS DISTINCT FROM e.new_status OR s.lifecycle_sequence IS DISTINCT FROM e.sequence")
        .fetch_one(pool).await.unwrap();
    assert_eq!(mismatches, 0);
}
