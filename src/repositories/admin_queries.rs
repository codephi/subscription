use chrono::Utc;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::admin_queries::{
        CreateWorkspaceRequest, CustomerPlanPageResponse, WorkspacePageResponse,
        WorkspaceProjectionResponse,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
    /// Create an administratively initialized workspace; e.g. `repo.create_workspace(&request).await`.
    pub async fn create_workspace(
        &self,
        request: &CreateWorkspaceRequest,
    ) -> ApiResult<WorkspaceProjectionResponse> {
        let workspace = NewAdminWorkspace::new();
        let mut transaction = self.pool().begin().await?;
        insert_admin_workspace(&mut transaction, &workspace, &request.actor_reference).await?;
        transaction.commit().await?;
        self.find_workspace_projection(workspace.workspace_id).await
    }

    /// Page workspace projections by stable ID; e.g. `repo.list_workspace_projections(None, 20).await`.
    pub async fn list_workspace_projections(
        &self,
        cursor: Option<Uuid>,
        limit: i64,
    ) -> ApiResult<WorkspacePageResponse> {
        let rows = sqlx::query("SELECT * FROM workspace_projections WHERE ($1::uuid IS NULL OR workspace_id > $1) ORDER BY workspace_id LIMIT $2")
            .bind(cursor).bind(limit + 1).fetch_all(&self.pool()).await?;
        let has_more = rows.len() as i64 > limit;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(workspace_from_row)
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| items.last().map(|item| item.workspace_id))
            .flatten();
        Ok(WorkspacePageResponse { items, next_cursor })
    }

    /// Read a workspace projection; e.g. `repo.find_workspace_projection(id).await`.
    pub async fn find_workspace_projection(
        &self,
        id: Uuid,
    ) -> ApiResult<WorkspaceProjectionResponse> {
        let row = sqlx::query("SELECT * FROM workspace_projections WHERE workspace_id=$1")
            .bind(id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| {
                ApiError::not_found(
                    "workspace_not_found",
                    format!("workspace {id} does not exist"),
                )
            })?;
        Ok(workspace_from_row(&row))
    }

    /// Page customer plans by stable ID; e.g. `repo.list_workspace_customer_plans(id, None, 20).await`.
    pub async fn list_workspace_customer_plans(
        &self,
        workspace_id: Uuid,
        cursor: Option<Uuid>,
        limit: i64,
    ) -> ApiResult<CustomerPlanPageResponse> {
        let ids = sqlx::query_scalar::<_, Uuid>("SELECT customer_plan_id FROM customer_plans WHERE customer_id=$1 AND ($2::uuid IS NULL OR customer_plan_id > $2) ORDER BY customer_plan_id LIMIT $3")
            .bind(workspace_id).bind(cursor).bind(limit + 1).fetch_all(&self.pool()).await?;
        let has_more = ids.len() as i64 > limit;
        let mut items = Vec::with_capacity(ids.len().min(limit as usize));
        for id in ids.iter().take(limit as usize) {
            items.push(self.find_customer_plan(workspace_id, *id).await?);
        }
        let next_cursor = has_more.then(|| ids[limit as usize - 1]);
        Ok(CustomerPlanPageResponse { items, next_cursor })
    }
}

struct NewAdminWorkspace {
    workspace_id: Uuid,
    event_id: Uuid,
    outbox_event_id: Uuid,
    correlation_id: Uuid,
    occurred_at: chrono::DateTime<Utc>,
    envelope: serde_json::Value,
    domain_event: serde_json::Value,
}

impl NewAdminWorkspace {
    fn new() -> Self {
        let workspace_id = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let outbox_event_id = Uuid::new_v4();
        let correlation_id = Uuid::new_v4();
        let occurred_at = Utc::now();
        Self {
            workspace_id,
            event_id,
            outbox_event_id,
            correlation_id,
            occurred_at,
            envelope: created_envelope(workspace_id, event_id, correlation_id, occurred_at),
            domain_event: projection_event(
                workspace_id,
                event_id,
                outbox_event_id,
                correlation_id,
                occurred_at,
            ),
        }
    }
}

fn created_envelope(
    workspace_id: Uuid,
    event_id: Uuid,
    correlation_id: Uuid,
    occurred_at: chrono::DateTime<Utc>,
) -> serde_json::Value {
    json!({"event_id":event_id,"event_type":"workspace.created","schema_version":1,
        "aggregate_id":workspace_id,"sequence":1,"occurred_at":occurred_at,
        "workspace_id":workspace_id,"correlation_id":correlation_id,"causation_id":null,
        "payload":{"workspace_id":workspace_id}})
}

fn projection_event(
    workspace_id: Uuid,
    event_id: Uuid,
    outbox_event_id: Uuid,
    correlation_id: Uuid,
    occurred_at: chrono::DateTime<Utc>,
) -> serde_json::Value {
    json!({"event_id":outbox_event_id,"event_type":"workspace.projection_updated",
        "schema_version":1,"aggregate_type":"workspace","aggregate_id":workspace_id,
        "sequence":1,"occurred_at":occurred_at,"workspace_id":workspace_id,
        "correlation_id":correlation_id,"causation_id":event_id,
        "payload":{"operational_status":"CREATED"}})
}

async fn insert_admin_workspace(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace: &NewAdminWorkspace,
    actor_reference: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workspace_projections (workspace_id, operational_status, external_sequence, external_occurred_at, last_event_id) VALUES ($1,'CREATED',1,$2,$3)")
        .bind(workspace.workspace_id).bind(workspace.occurred_at).bind(workspace.event_id).execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO integration_inbox (event_id,workspace_id,event_type,schema_version,aggregate_id,external_sequence,occurred_at,correlation_id,payload,processing_status,processed_at) VALUES ($1,$2,'workspace.created',1,$2,1,$3,$4,$5,'PROCESSED',now())")
        .bind(workspace.event_id).bind(workspace.workspace_id).bind(workspace.occurred_at).bind(workspace.correlation_id).bind(&workspace.envelope).execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence,workspace_id,correlation_id,causation_id,payload,occurred_at) VALUES ($1,'workspace.projection_updated','workspace',$2,1,$2,$3,$4,$5,$6)")
        .bind(workspace.outbox_event_id).bind(workspace.workspace_id).bind(workspace.correlation_id).bind(workspace.event_id).bind(&workspace.domain_event).bind(workspace.occurred_at).execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO audit_events (audit_event_id,workspace_id,actor_reference,action,resource_kind,resource_id,correlation_id,details) VALUES ($1,$2,$3,'workspace.admin_created','workspace_projection',$2,$4,$5)")
        .bind(Uuid::new_v4()).bind(workspace.workspace_id).bind(actor_reference).bind(workspace.correlation_id).bind(json!({"operational_status":"CREATED","sequence":1})).execute(&mut **transaction).await?;
    Ok(())
}

fn workspace_from_row(row: &sqlx::postgres::PgRow) -> WorkspaceProjectionResponse {
    WorkspaceProjectionResponse {
        workspace_id: row.get("workspace_id"),
        operational_status: row.get("operational_status"),
        external_sequence: row.get("external_sequence"),
        external_occurred_at: row.get("external_occurred_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}
