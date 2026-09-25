mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inbox_page_exposes_replay_references_without_payload() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let workspace_id = Uuid::new_v4();
    let event_id = Uuid::new_v4();
    sqlx::query("INSERT INTO integration_inbox (event_id,workspace_id,event_type,schema_version,aggregate_id,external_sequence,occurred_at,correlation_id,payload,processing_status) VALUES ($1,$2,'workspace.updated',1,$2,1,now(),$3,'{}'::jsonb,'QUARANTINED')")
        .bind(event_id).bind(workspace_id).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/admin/integration-inbox?workspace_id={workspace_id}&status=QUARANTINED"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = support::response_json(response).await;
    assert_eq!(page["items"][0]["event_id"], event_id.to_string());
    assert!(page["items"][0].get("payload").is_none());
}
