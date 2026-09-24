use uuid::Uuid;

use crate::{
    dto::admin_queries::{
        AdminPageQuery, CustomerPlanPageResponse, WorkspacePageResponse,
        WorkspaceProjectionResponse,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

/// List workspace projections for administration; e.g. `list_workspaces(&repo, query).await`.
pub async fn list_workspaces(
    repository: &DatabaseRepository,
    query: AdminPageQuery,
) -> ApiResult<WorkspacePageResponse> {
    repository
        .list_workspace_projections(query.cursor, page_limit(query.limit)?)
        .await
}

/// Read one workspace projection; e.g. `get_workspace(&repo, id).await`.
pub async fn get_workspace(
    repository: &DatabaseRepository,
    id: Uuid,
) -> ApiResult<WorkspaceProjectionResponse> {
    repository.find_workspace_projection(id).await
}

/// List a workspace's customer plans; e.g. `list_customer_plans(&repo, id, query).await`.
pub async fn list_customer_plans(
    repository: &DatabaseRepository,
    id: Uuid,
    query: AdminPageQuery,
) -> ApiResult<CustomerPlanPageResponse> {
    repository.find_workspace_projection(id).await?;
    repository
        .list_workspace_customer_plans(id, query.cursor, page_limit(query.limit)?)
        .await
}

fn page_limit(limit: Option<u16>) -> ApiResult<i64> {
    let limit = limit.unwrap_or(20);
    if (1..=100).contains(&limit) {
        return Ok(i64::from(limit));
    }
    Err(ApiError::unprocessable(
        "invalid_admin_page_limit",
        format!("limit {limit} must be between 1 and 100"),
    ))
}

#[cfg(test)]
mod tests {
    use super::page_limit;

    #[test]
    fn admin_page_limit_is_bounded() {
        assert_eq!(page_limit(None).unwrap(), 20);
        assert_eq!(page_limit(Some(100)).unwrap(), 100);
        assert!(page_limit(Some(0)).is_err());
        assert!(page_limit(Some(101)).is_err());
    }
}
