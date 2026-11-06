mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_entries_are_discoverable_and_paged() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    for id in [first, second] {
        sqlx::query("INSERT INTO products (product_id,name,usage_model,status) VALUES ($1,'Test','CREDIT_METERED','ACTIVE')")
            .bind(id).execute(&pool).await.unwrap();
    }
    let page = get_json(&router, "/v1/admin/catalog/products?limit=1").await;
    assert_eq!(page["items"][0]["id"], first.to_string());
    assert_eq!(page["next_cursor"], first.to_string());
    let next = get_json(
        &router,
        &format!("/v1/admin/catalog/products?cursor={first}&limit=1"),
    )
    .await;
    assert_eq!(next["items"][0]["id"], second.to_string());
    assert!(next["next_cursor"].is_null());
    for kind in [
        "items",
        "prices",
        "subscriptions",
        "plans",
        "on-demand",
        "policies",
    ] {
        assert_eq!(
            get_json(&router, &format!("/v1/admin/catalog/{kind}")).await["items"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
    assert_eq!(
        get_status(&router, "/v1/admin/catalog/products?limit=0").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        get_status(&router, "/v1/admin/catalog/invalid").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let subscription_id = Uuid::new_v4();
    let offer_id = Uuid::new_v4();
    sqlx::query("INSERT INTO subscriptions (subscription_id,name,subscription_model) VALUES ($1,'Test','CREDIT_STRICT')")
        .bind(subscription_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO on_demand_plans (on_demand_plan_id,subscription_id,name,price_amount_minor,currency,credit_units) VALUES ($1,$2,'Pack',100,'BRL',10)")
        .bind(offer_id).bind(subscription_id).execute(&pool).await.unwrap();
    assert_eq!(
        get_json(&router, "/v1/admin/catalog/on-demand").await["items"][0]["id"],
        offer_id.to_string()
    );
    assert_eq!(
        get_json(&router, &format!("/v1/on-demand-plans/{offer_id}")).await["name"],
        "Pack"
    );
}

async fn get_json(router: &axum::Router, uri: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    support::response_json(response).await
}

async fn get_status(router: &axum::Router, uri: &str) -> StatusCode {
    router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}
