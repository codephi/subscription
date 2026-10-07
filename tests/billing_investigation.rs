mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn billing_queues_filter_page_and_open_details() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let account_id = Uuid::new_v4();
    sqlx::query("INSERT INTO account_projections (account_id,operational_status,external_sequence,external_occurred_at,last_event_id) VALUES ($1,'ACTIVE',1,now(),$2)")
        .bind(account_id).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    for id in [first, second] {
        sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence,account_id,correlation_id,payload) VALUES ($1,'test.created','test',$2,1,$3,$4,'{}'::jsonb)")
            .bind(id).bind(id).bind(account_id).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    }
    let page = get_json(
        &router,
        &format!("/v1/admin/billing/records/outbox?account_id={account_id}&status=PENDING&limit=1"),
    )
    .await;
    assert_eq!(page["items"][0]["id"], first.to_string());
    assert_eq!(page["next_cursor"], first.to_string());
    let next = get_json(
        &router,
        &format!("/v1/admin/billing/records/outbox?cursor={first}&limit=1"),
    )
    .await;
    assert_eq!(next["items"][0]["id"], second.to_string());
    assert!(next["next_cursor"].is_null());
    let detail = get_json(
        &router,
        &format!("/v1/admin/billing/records/outbox/{first}"),
    )
    .await;
    assert_eq!(detail["account_id"], account_id.to_string());
    assert_eq!(detail["status"], "PENDING");
    for kind in [
        "collections",
        "attempts",
        "payments",
        "webhooks",
        "unmatched",
    ] {
        assert_eq!(
            get_json(&router, &format!("/v1/admin/billing/records/{kind}")).await["items"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
    assert_eq!(
        get_status(&router, "/v1/admin/billing/records/outbox?limit=0").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        get_status(&router, "/v1/admin/billing/records/invalid").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        get_status(
            &router,
            &format!("/v1/admin/billing/records/outbox/{}", Uuid::new_v4())
        )
        .await,
        StatusCode::NOT_FOUND
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
