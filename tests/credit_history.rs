#[path = "support/credit_fixture.rs"]
mod credit_fixture;
mod support;

use credit_fixture::CreditFixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_credit_history_column_and_lot_origin_reject_mutation() {
    let fixture = CreditFixture::new().await;
    fixture.grant("immutable", 30).await.unwrap();
    for table in [
        "direct_credits",
        "customer_wallet_entries",
        "wallet_transaction_references",
    ] {
        assert_all_columns_protected(&fixture, table).await;
        assert_history_delete_rejected(&fixture, table).await;
    }
    for assignment in [
        "credit_lot_id=gen_random_uuid()",
        "customer_id=gen_random_uuid()",
        "granting_entry_id=gen_random_uuid()",
        "original_credit_units=original_credit_units+1",
        "created_at=created_at+interval '1 second'",
    ] {
        let before = fixture.snapshot().await;
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE credit_lots SET ");
        query.push(assignment);
        let error = query.build().execute(&fixture.pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        assert_eq!(fixture.snapshot().await, before);
    }
    assert_history_delete_rejected(&fixture, "credit_lots").await;
}

async fn assert_all_columns_protected(fixture: &CreditFixture, table: &'static str) {
    let columns: Vec<String> = sqlx::query_scalar("SELECT column_name FROM information_schema.columns WHERE table_schema='public' AND table_name=$1 ORDER BY ordinal_position")
        .bind(table).fetch_all(&fixture.pool).await.unwrap();
    assert!(!columns.is_empty());
    let before = fixture.snapshot().await;
    for column in columns {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE ");
        query
            .push(table)
            .push(" SET \"")
            .push(&column)
            .push("\"=\"")
            .push(&column)
            .push("\"");
        let error = query.build().execute(&fixture.pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    assert_eq!(fixture.snapshot().await, before);
}

async fn assert_history_delete_rejected(fixture: &CreditFixture, table: &'static str) {
    let before = fixture.snapshot().await;
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("DELETE FROM ");
    query.push(table);
    let error = query.build().execute(&fixture.pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert_eq!(fixture.snapshot().await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_lot_origin_migration_round_trips_and_preserves_rows() {
    let fixture = CreditFixture::new().await;
    fixture.grant("migration", 30).await.unwrap();
    let before = fixture.snapshot().await;
    sqlx::raw_sql(include_str!(
        "../migrations/202609100001_credit_lot_origin.down.sql"
    ))
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/202609100001_credit_lot_origin.up.sql"
    ))
    .execute(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(fixture.snapshot().await, before);
    assert_history_delete_rejected(&fixture, "credit_lots").await;
    sqlx::query(
        "UPDATE credit_lots SET remaining_credit_units=20,source_kind='ON_DEMAND',expires_at=NULL",
    )
    .execute(&fixture.pool)
    .await
    .unwrap();
    let origin: (i64, i64) =
        sqlx::query_as("SELECT original_credit_units,remaining_credit_units FROM credit_lots")
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(origin, (30, 20));
}
