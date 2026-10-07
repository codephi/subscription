use axum::http::{header, HeaderMap, HeaderValue};
use sqlx::sqlite::SqlitePoolOptions;
use tasklab::{api::SubscriptionClient, auth, database, state::AppState};

#[tokio::test]
async fn seeded_admin_authenticates_and_registered_accounts_are_isolated() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("memory sqlite");
    database::initialize(&pool)
        .await
        .expect("schema and admin seed");
    let seeded = auth::authenticate(&pool, "admin", "admin")
        .await
        .expect("seeded demo login");
    let first = auth::register(&pool, "alice", "a-strong-password")
        .await
        .expect("first registration");
    let second = auth::register(&pool, "bob", "another-strong-password")
        .await
        .expect("second registration");
    assert_ne!(first.account_id, second.account_id);
    assert_ne!(first.user_id, seeded.user_id);
    assert!(auth::authenticate(&pool, "alice", "wrong-password")
        .await
        .is_err());
    assert!(auth::register(&pool, "alice", "different-password")
        .await
        .is_err());
}

#[tokio::test]
async fn session_cookie_is_http_only_and_revocation_ends_the_session() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("memory sqlite");
    database::initialize(&pool)
        .await
        .expect("schema and admin seed");
    let user = auth::authenticate(&pool, "admin", "admin")
        .await
        .expect("seeded demo login");
    let set_cookie = auth::create_session(&pool, &user)
        .await
        .expect("new session");
    let cookie_header = set_cookie.to_str().expect("cookie value");
    assert!(cookie_header.contains("HttpOnly"));
    let request_cookie = cookie_header.split(';').next().expect("cookie pair");
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(request_cookie).expect("request cookie"),
    );
    let state = AppState::new(
        pool.clone(),
        SubscriptionClient::new("http://127.0.0.1:3000").expect("API URL"),
        "fake-secret".into(),
    );
    assert_eq!(
        auth::current_user(&state, &headers)
            .await
            .expect("valid session")
            .user_id,
        user.user_id
    );
    auth::delete_session(&pool, &headers)
        .await
        .expect("session revocation");
    assert!(auth::current_user(&state, &headers).await.is_err());
}
