use uuid::Uuid;

use crate::{
    dto::admin_queries::{
        AdminPageQuery, CreateWorkspaceRequest, CustomerPlanPageResponse, WorkspacePageResponse,
        WorkspaceProjectionResponse,
    },
    dto::events::WorkspaceEventResponse,
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

/// Create a workspace from the administrative panel; e.g. `create_workspace(repo, request).await`.
pub async fn create_workspace(
    repository: &DatabaseRepository,
    mut request: CreateWorkspaceRequest,
) -> ApiResult<WorkspaceProjectionResponse> {
    request.actor_reference = request.actor_reference.trim().to_string();
    validate_workspace_actor(&request.actor_reference)?;
    let workspace = repository.create_workspace(&request).await?;
    crate::services::billing_integrations::provision_workspace_defaults(
        repository,
        workspace.workspace_id,
    )
    .await?;
    Ok(workspace)
}

fn validate_workspace_actor(actor_reference: &str) -> ApiResult<()> {
    if actor_reference.is_empty() || actor_reference.len() > 255 {
        return Err(ApiError::unprocessable(
            "invalid_actor_reference",
            format!("actor_reference {actor_reference:?} must contain 1 to 255 characters"),
        ));
    }
    Ok(())
}

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

/// Terminate a workspace from administration; e.g. `terminate_workspace(repo, id).await`.
pub async fn terminate_workspace(
    repository: &DatabaseRepository,
    id: Uuid,
) -> ApiResult<WorkspaceEventResponse> {
    repository.terminate_workspace(id).await
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
    use super::{page_limit, validate_workspace_actor};

    #[test]
    fn admin_page_limit_is_bounded() {
        assert_eq!(page_limit(None).unwrap(), 20);
        assert_eq!(page_limit(Some(100)).unwrap(), 100);
        assert!(page_limit(Some(0)).is_err());
        assert!(page_limit(Some(101)).is_err());
    }

    #[test]
    fn workspace_actor_reference_must_be_present_and_bounded() {
        assert!(validate_workspace_actor("operator@example.com").is_ok());
        assert!(validate_workspace_actor("").is_err());
        assert!(validate_workspace_actor(&"x".repeat(256)).is_err());
    }
}
