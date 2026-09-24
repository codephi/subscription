use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::admin_queries::{
        CustomerPlanPageResponse, WorkspacePageResponse, WorkspaceProjectionResponse,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
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
