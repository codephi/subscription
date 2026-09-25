mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_page_filters_workspace_and_action() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let workspace_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();
    for (id, workspace, action) in [
        (Uuid::from_u128(1), workspace_id, "CREDIT"),
        (Uuid::from_u128(2), other_id, "CREDIT"),
        (Uuid::from_u128(3), workspace_id, "PLAN"),
    ] {
        sqlx::query("INSERT INTO audit_events (audit_event_id,workspace_id,action,resource_kind,correlation_id) VALUES ($1,$2,$3,'test',$4)")
            .bind(id).bind(workspace).bind(action).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    }
    let response = get(
        &router,
        &format!("/v1/admin/audit-events?workspace_id={workspace_id}&action=CREDIT"),
    )
    .await;
    assert_eq!(response["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        response["items"][0]["audit_event_id"],
        Uuid::from_u128(1).to_string()
    );
    let response = get(&router, "/v1/admin/audit-events?limit=1").await;
    assert_eq!(response["next_cursor"], Uuid::from_u128(1).to_string());
    let status = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/admin/audit-events?limit=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

async fn get(router: &axum::Router, uri: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    support::response_json(response).await
}
