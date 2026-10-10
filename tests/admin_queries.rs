mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_account_and_plan_reads_page_without_cross_account_data() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    for id in [first, second] {
        insert_account(&pool, id).await;
    }
    let first_page = get_json(&router, "/v1/admin/accounts?limit=1").await;
    assert_eq!(first_page["items"][0]["account_id"], first.to_string());
    assert_eq!(first_page["next_cursor"], first.to_string());
    let next = get_json(
        &router,
        &format!("/v1/admin/accounts?limit=1&cursor={first}"),
    )
    .await;
    assert_eq!(next["items"][0]["account_id"], second.to_string());
    assert!(next["next_cursor"].is_null());

    let mut plan_ids = [
        insert_customer_plan(&pool, first).await,
        insert_customer_plan(&pool, first).await,
    ];
    plan_ids.sort();
    let plans = get_json(
        &router,
        &format!("/v1/admin/accounts/{first}/customer-plans?limit=1"),
    )
    .await;
    assert_eq!(
        plans["items"][0]["customer_plan_id"],
        plan_ids[0].to_string()
    );
    assert_eq!(plans["next_cursor"], plan_ids[0].to_string());
    let second_plan_page = get_json(
        &router,
        &format!(
            "/v1/admin/accounts/{first}/customer-plans?limit=1&cursor={}",
            plan_ids[0]
        ),
    )
    .await;
    assert_eq!(
        second_plan_page["items"][0]["customer_plan_id"],
        plan_ids[1].to_string()
    );
    assert!(second_plan_page["next_cursor"].is_null());
    let other = get_json(
        &router,
        &format!("/v1/admin/accounts/{second}/customer-plans"),
    )
    .await;
    assert_eq!(other["items"].as_array().unwrap().len(), 0);
    assert_eq!(
        get_status(&router, "/v1/admin/accounts?limit=0").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        get_status(&router, &format!("/v1/admin/accounts/{}", Uuid::new_v4())).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_account_creation_records_projection_event_and_actor_atomically() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let response = router
        .oneshot(
            Request::post("/v1/admin/accounts")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"actor_reference":"ops@example.com"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = support::response_json(response).await;
    let account_id = created["account_id"].as_str().unwrap();
    assert_eq!(created["operational_status"], "CREATED");
    assert_eq!(created["external_sequence"], 1);
    let audit_actor: String = sqlx::query_scalar(
        "SELECT actor_reference FROM audit_events WHERE account_id=$1 AND action='account.admin_created'",
    )
    .bind(Uuid::parse_str(account_id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_actor, "ops@example.com");
    let inbox_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM integration_inbox WHERE account_id=$1 AND processing_status='PROCESSED'",
    )
    .bind(Uuid::parse_str(account_id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(inbox_count, 1);
    let outbox_type: String =
        sqlx::query_scalar("SELECT payload->>'event_type' FROM outbox_events WHERE account_id=$1")
            .bind(Uuid::parse_str(account_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(outbox_type, "account.projection_updated");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_can_terminate_account_without_removing_its_history() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let account_id = Uuid::from_u128(3);
    insert_account(&pool, account_id).await;

    let response = router
        .clone()
        .oneshot(
            Request::post(format!("/v1/admin/accounts/{account_id}/terminate"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result = support::response_json(response).await;
    assert_eq!(result["account_status"], "TERMINATED");
    assert_eq!(result["external_sequence"], 3);

    let history_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM integration_inbox WHERE account_id=$1 AND event_type='account.terminated' AND processing_status='PROCESSED'",
    )
    .bind(account_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(history_count, 1);
    let status: String = sqlx::query_scalar(
        "SELECT operational_status FROM account_projections WHERE account_id=$1",
    )
    .bind(account_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "TERMINATED");

    let repeated = router
        .oneshot(
            Request::post(format!("/v1/admin/accounts/{account_id}/terminate"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(repeated.status(), StatusCode::CONFLICT);
}

async fn insert_account(pool: &sqlx::PgPool, id: Uuid) {
    sqlx::query("INSERT INTO account_projections (account_id,operational_status,external_sequence,external_occurred_at,last_event_id) VALUES ($1,'ACTIVE',2,now(),$2)")
        .bind(id).bind(Uuid::new_v4()).execute(pool).await.unwrap();
}

async fn insert_customer_plan(pool: &sqlx::PgPool, account_id: Uuid) -> Uuid {
    let subscription_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    let customer_plan_id = Uuid::new_v4();
    sqlx::query("INSERT INTO subscriptions (subscription_id,name,subscription_model) VALUES ($1,'Test','CREDIT_STRICT')")
        .bind(subscription_id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO subscription_plan_versions (plan_version_id,subscription_id,name,commercial_model,recurrence,admission_policy,granted_credit_units) VALUES ($1,$2,'Test','FREE','MONTHLY','OPEN',0)")
        .bind(plan_id).bind(subscription_id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO customer_plans (customer_plan_id,customer_id,plan_version_id,commercial_status,activation_status,renewal_status,anchor_at) VALUES ($1,$2,$3,'ACTIVE','ACTIVATED','CURRENT',now())")
        .bind(customer_plan_id).bind(account_id).bind(plan_id).execute(pool).await.unwrap();
    customer_plan_id
}

async fn get_json(router: &axum::Router, uri: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
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
