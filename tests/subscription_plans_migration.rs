mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscription_plans_migration_round_trips() {
    let (_, pool) = support::setup_router_with_options(false, None).await;
    let mut connection = pool.acquire().await.expect("test connection");
    sqlx::query("DROP SCHEMA IF EXISTS subscription_plan_migration_round_trip CASCADE")
        .execute(&mut *connection)
        .await
        .expect("reset isolated schema");
    sqlx::query("CREATE SCHEMA subscription_plan_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("create isolated schema");
    sqlx::query("SET search_path TO subscription_plan_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("select isolated schema");
    for migration in [
        include_str!("../migrations/202601020000_init.sql"),
        include_str!("../migrations/202609030001_foundation.up.sql"),
        include_str!("../migrations/202609030002_catalog.up.sql"),
        include_str!("../migrations/202609030003_wallet_provisioning.up.sql"),
        include_str!("../migrations/202609030004_credit_ledger.up.sql"),
        include_str!("../migrations/202609040005_subscription_plans.up.sql"),
    ] {
        apply_migration(&mut connection, migration).await;
    }
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('customer_plans') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("customer plans table exists");
    assert!(exists);
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609040005_subscription_plans.down.sql"),
    )
    .await;
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('customer_plans') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("customer plans table removed");
    assert!(!exists);
}

async fn apply_migration(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    sql: &'static str,
) {
    sqlx::raw_sql(sql)
        .execute(&mut **connection)
        .await
        .expect("apply migration");
}
