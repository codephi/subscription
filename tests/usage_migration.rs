mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_metering_migrations_round_trip() {
    let (_, pool) = support::setup_router_with_options(false, None).await;
    let mut connection = pool.acquire().await.expect("test connection");
    sqlx::query("DROP SCHEMA IF EXISTS usage_migration_round_trip CASCADE")
        .execute(&mut *connection)
        .await
        .expect("reset isolated schema");
    sqlx::query("CREATE SCHEMA usage_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("create isolated schema");
    sqlx::query("SET search_path TO usage_migration_round_trip")
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
        include_str!("../migrations/202609040006_usage_metering.up.sql"),
        include_str!("../migrations/202609040007_usage_pricing_details.up.sql"),
    ] {
        apply_migration(&mut connection, migration).await;
    }
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('billing_blocks') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("billing blocks table exists");
    assert!(exists);
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609040007_usage_pricing_details.down.sql"),
    )
    .await;
    let tier_column: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema=current_schema() \
         AND table_name='billing_blocks' AND column_name='tier_position')",
    )
    .fetch_one(&mut *connection)
    .await
    .expect("tier column state");
    assert!(!tier_column);
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609040006_usage_metering.down.sql"),
    )
    .await;
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('billing_blocks') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("billing blocks table removed");
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
