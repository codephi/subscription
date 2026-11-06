mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_fixtures_have_clean_databases_and_independent_connections() {
    let (_, first) = support::setup_router_with_options(false, None).await;
    sqlx::query("CREATE TABLE fixture_isolation_probe (value integer)")
        .execute(&first)
        .await
        .expect("first database marker");
    let (_, second) = support::setup_router_with_options(false, None).await;
    let marker: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('fixture_isolation_probe')::text")
            .fetch_one(&second)
            .await
            .expect("second database");
    assert!(
        marker.is_none(),
        "fixture must not inherit another test's state"
    );
    let mut left = first.acquire().await.expect("first connection");
    let mut right = first.acquire().await.expect("independent connection");
    let left_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *left)
        .await
        .unwrap();
    let right_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *right)
        .await
        .unwrap();
    assert_ne!(left_pid, right_pid);
}
