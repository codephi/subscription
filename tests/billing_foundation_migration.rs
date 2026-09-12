mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn billing_foundation_migrations_round_trip() {
    let (_, pool) = support::setup_router_with_options(false, None).await;
    let mut connection = pool.acquire().await.expect("test connection");
    sqlx::query("DROP SCHEMA IF EXISTS billing_migration_round_trip CASCADE")
        .execute(&mut *connection)
        .await
        .expect("reset isolated schema");
    sqlx::query("CREATE SCHEMA billing_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("create isolated schema");
    sqlx::query("SET search_path TO billing_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("select isolated schema");
    for migration in prerequisite_migrations() {
        apply_migration(&mut connection, migration).await;
    }
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609040008_billing_foundation.up.sql"),
    )
    .await;
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables WHERE table_schema=current_schema() \
         AND table_name IN ('billing_connections','payment_method_bindings','collection_requests', \
         'collection_attempts','billing_payments','billing_webhook_inbox')",
    )
    .fetch_one(&mut *connection)
    .await
    .expect("billing tables");
    assert_eq!(tables, 6);
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609040009_billing_credit_references.up.sql"),
    )
    .await;
    let reference_exists: bool =
        sqlx::query_scalar("SELECT to_regclass('billing_credit_grant_references') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await
            .expect("billing credit reference table exists");
    assert!(reference_exists);
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609120001_billing_scope_integrity.up.sql"),
    )
    .await;
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609120002_subscription_payment_window.up.sql"),
    )
    .await;
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609120002_subscription_payment_window.down.sql"),
    )
    .await;
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609120001_billing_scope_integrity.down.sql"),
    )
    .await;
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609040009_billing_credit_references.down.sql"),
    )
    .await;
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609040008_billing_foundation.down.sql"),
    )
    .await;
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('collection_requests') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("collection request table removed");
    assert!(!exists);
}

fn prerequisite_migrations() -> [&'static str; 8] {
    [
        include_str!("../migrations/202601020000_init.sql"),
        include_str!("../migrations/202609030001_foundation.up.sql"),
        include_str!("../migrations/202609030002_catalog.up.sql"),
        include_str!("../migrations/202609030003_wallet_provisioning.up.sql"),
        include_str!("../migrations/202609030004_credit_ledger.up.sql"),
        include_str!("../migrations/202609040005_subscription_plans.up.sql"),
        include_str!("../migrations/202609040006_usage_metering.up.sql"),
        include_str!("../migrations/202609040007_usage_pricing_details.up.sql"),
    ]
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
