use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::admin_queries::{
        AdminPageQuery, CreateWorkspaceRequest, CustomerPlanPageResponse, WorkspacePageResponse,
        WorkspaceProjectionResponse,
    },
    dto::events::WorkspaceEventResponse,
    error::{ApiResult, ErrorResponse},
    services::admin_queries,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_workspace))
        .routes(routes!(list_workspaces))
        .routes(routes!(get_workspace))
        .routes(routes!(terminate_workspace))
        .routes(routes!(list_customer_plans))
}

#[utoipa::path(post, path = "/v1/admin/workspaces", tag = "Operations", request_body = CreateWorkspaceRequest,
    responses((status = 201, body = WorkspaceProjectionResponse), (status = 422, body = ErrorResponse)))]
async fn create_workspace(
    State(state): State<AppState>,
    Json(request): Json<CreateWorkspaceRequest>,
) -> ApiResult<(StatusCode, Json<WorkspaceProjectionResponse>)> {
    let workspace = admin_queries::create_workspace(&state.database(), request).await?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

#[utoipa::path(get, path = "/v1/admin/workspaces", tag = "Operations", params(AdminPageQuery),
    responses((status = 200, body = WorkspacePageResponse), (status = 422, body = ErrorResponse)))]
async fn list_workspaces(
    State(state): State<AppState>,
    Query(query): Query<AdminPageQuery>,
) -> ApiResult<Json<WorkspacePageResponse>> {
    Ok(Json(
        admin_queries::list_workspaces(&state.database(), query).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/workspaces/{workspace_id}", tag = "Operations", params(("workspace_id" = Uuid, Path)),
    responses((status = 200, body = WorkspaceProjectionResponse), (status = 404, body = ErrorResponse)))]
async fn get_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<WorkspaceProjectionResponse>> {
    Ok(Json(
        admin_queries::get_workspace(&state.database(), id).await?,
    ))
}

#[utoipa::path(post, path = "/v1/admin/workspaces/{workspace_id}/terminate", tag = "Operations",
    params(("workspace_id" = Uuid, Path)),
    responses((status = 200, body = WorkspaceEventResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse)))]
async fn terminate_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<WorkspaceEventResponse>> {
    Ok(Json(
        admin_queries::terminate_workspace(&state.database(), id).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/workspaces/{workspace_id}/customer-plans", tag = "Operations",
    params(("workspace_id" = Uuid, Path), AdminPageQuery),
    responses((status = 200, body = CustomerPlanPageResponse), (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse)))]
async fn list_customer_plans(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<AdminPageQuery>,
) -> ApiResult<Json<CustomerPlanPageResponse>> {
    Ok(Json(
        admin_queries::list_customer_plans(&state.database(), id, query).await?,
    ))
}
