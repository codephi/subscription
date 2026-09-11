#![allow(dead_code)]

use axum::{body::Body, http::Request, response::Response, Router};
use serde_json::{json, Value};
use sqlx::PgPool;
use subscription::{
    dto::{
        credits::{DirectCreditRequest, DirectCreditResponse},
        events::WorkspaceEventEnvelope,
    },
    error::ApiResult,
    repositories::database::DatabaseRepository,
    services::{credits, workspace_events},
};
use tower::ServiceExt;
use uuid::Uuid;

pub struct CreditFixture {
    pub router: Router,
    pub pool: PgPool,
    pub repository: DatabaseRepository,
    pub workspace_id: Uuid,
}

impl CreditFixture {
    pub async fn new() -> Self {
        let (router, pool) = super::support::setup_router_with_options(false, None).await;
        let repository = DatabaseRepository::new(pool.clone());
        let fixture = Self {
            router,
            pool,
            repository,
            workspace_id: Uuid::new_v4(),
        };
        fixture.event("workspace.created", 1).await;
        fixture.event("workspace.activated", 2).await;
        fixture
    }

    pub async fn event(&self, kind: &str, sequence: i64) {
        let envelope: WorkspaceEventEnvelope = serde_json::from_value(json!({
            "event_id":Uuid::new_v4(),"event_type":kind,"schema_version":1,
            "aggregate_id":self.workspace_id,"workspace_id":self.workspace_id,"sequence":sequence,
            "occurred_at":chrono::Utc::now(),"correlation_id":Uuid::new_v4(),
            "payload":{"workspace_id":self.workspace_id}
        }))
        .unwrap();
        workspace_events::process_workspace_event(&self.repository, envelope)
            .await
            .unwrap();
    }

    pub async fn grant(&self, identifier: &str, units: i64) -> ApiResult<DirectCreditResponse> {
        credits::grant_direct_credit(
            &self.repository,
            self.workspace_id,
            identifier,
            credit_request(identifier, units),
        )
        .await
    }

    pub fn post_request(&self, key: &str, body: Value) -> Request<Body> {
        Request::post(format!(
            "/v1/workspaces/{}/credits/direct",
            self.workspace_id
        ))
        .header("content-type", "application/json")
        .header("idempotency-key", key)
        .body(Body::from(body.to_string()))
        .unwrap()
    }

    pub async fn post(&self, key: &str, body: Value) -> Response {
        self.router
            .clone()
            .oneshot(self.post_request(key, body))
            .await
            .unwrap()
    }

    pub async fn get(&self, uri: &str) -> Response {
        self.router
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    pub async fn snapshot(&self) -> Vec<Value> {
        let mut snapshot = Vec::new();
        for table in CREDIT_TABLES {
            let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "SELECT COALESCE(jsonb_agg(r ORDER BY r::text),'[]') FROM (SELECT to_jsonb(t) r FROM ");
            query.push(*table).push(" t) snapshot");
            snapshot.push(
                query
                    .build_query_scalar()
                    .fetch_one(&self.pool)
                    .await
                    .unwrap(),
            );
        }
        snapshot
    }
}

pub fn credit_request(identifier: &str, units: i64) -> DirectCreditRequest {
    serde_json::from_value(credit_body(identifier, units)).unwrap()
}

pub fn credit_body(identifier: &str, units: i64) -> Value {
    json!({"transaction_id":identifier,"credit_units":units.to_string(),
        "description":"credit recovery test","external_reference":"external-order",
        "metadata":{"private_note":"must not appear in outbound events"}})
}

const CREDIT_TABLES: &[&str] = &[
    "wallets",
    "customer_wallets",
    "item_wallets",
    "wallet_effective_states",
    "wallet_provisioning",
    "direct_credits",
    "customer_wallet_entries",
    "credit_lots",
    "wallet_transaction_references",
    "idempotency_records",
    "transaction_reservations",
    "audit_events",
    "outbox_events",
];
