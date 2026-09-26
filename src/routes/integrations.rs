use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::billing::{
        CreateStripeIntegrationRequest, IntegrationProviderResponse, StripeIntegrationTestResponse,
        UpdateStripeIntegrationRequest, WorkspaceIntegrationResponse,
    },
    error::{ApiResult, ErrorResponse},
    services::billing_integrations as integrations,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_providers))
        .routes(routes!(list_workspace_integrations))
        .routes(routes!(get_workspace_integration))
        .routes(routes!(create_workspace_integration))
        .routes(routes!(update_workspace_integration))
        .routes(routes!(test_workspace_integration))
}

#[utoipa::path(get, path = "/v1/admin/integrations/providers", tag = "Operations",
    responses((status = 200, body = [IntegrationProviderResponse])))]
async fn list_providers() -> Json<Vec<IntegrationProviderResponse>> {
    Json(integrations::providers())
}

#[utoipa::path(get, path = "/v1/admin/workspaces/{workspace_id}/integrations", tag = "Operations",
    params(("workspace_id" = Uuid, Path)),
    responses((status = 200, body = [WorkspaceIntegrationResponse])))]
async fn list_workspace_integrations(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<Vec<WorkspaceIntegrationResponse>>> {
    let mut result = integrations::list(&state.database(), workspace_id).await?;
    result
        .iter_mut()
        .for_each(|item| attach_webhook_url(&state, item));
    Ok(Json(result))
}

#[utoipa::path(get, path = "/v1/admin/workspaces/{workspace_id}/integrations/{connection_id}", tag = "Operations",
    params(("workspace_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    responses((status = 200, body = WorkspaceIntegrationResponse), (status = 404, body = ErrorResponse)))]
async fn get_workspace_integration(
    State(state): State<AppState>,
    Path((workspace_id, connection_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<WorkspaceIntegrationResponse>> {
    let mut result = integrations::get(&state.database(), workspace_id, connection_id).await?;
    attach_webhook_url(&state, &mut result);
    Ok(Json(result))
}

#[utoipa::path(post, path = "/v1/admin/workspaces/{workspace_id}/integrations", tag = "Operations",
    params(("workspace_id" = Uuid, Path)), request_body = CreateStripeIntegrationRequest,
    responses((status = 201, body = WorkspaceIntegrationResponse), (status = 422, body = ErrorResponse)))]
async fn create_workspace_integration(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<CreateStripeIntegrationRequest>,
) -> ApiResult<(StatusCode, Json<WorkspaceIntegrationResponse>)> {
    let mut result = integrations::create_stripe(&state.database(), workspace_id, &request).await?;
    attach_webhook_url(&state, &mut result);
    Ok((StatusCode::CREATED, Json(result)))
}

#[utoipa::path(patch, path = "/v1/admin/workspaces/{workspace_id}/integrations/{connection_id}", tag = "Operations",
    params(("workspace_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    request_body = UpdateStripeIntegrationRequest,
    responses((status = 200, body = WorkspaceIntegrationResponse), (status = 409, body = ErrorResponse)))]
async fn update_workspace_integration(
    State(state): State<AppState>,
    Path((workspace_id, connection_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateStripeIntegrationRequest>,
) -> ApiResult<Json<WorkspaceIntegrationResponse>> {
    let mut result =
        integrations::update_stripe(&state.database(), workspace_id, connection_id, &request)
            .await?;
    attach_webhook_url(&state, &mut result);
    Ok(Json(result))
}

#[utoipa::path(post, path = "/v1/admin/workspaces/{workspace_id}/integrations/{connection_id}/test", tag = "Operations",
    params(("workspace_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    responses((status = 200, body = StripeIntegrationTestResponse), (status = 422, body = ErrorResponse)))]
async fn test_workspace_integration(
    State(state): State<AppState>,
    Path((workspace_id, connection_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<StripeIntegrationTestResponse>> {
    Ok(Json(
        integrations::test_stripe(&state.database(), workspace_id, connection_id).await?,
    ))
}

fn attach_webhook_url(state: &AppState, integration: &mut WorkspaceIntegrationResponse) {
    integration.webhook_url = state
        .public_api_base_url()
        .map(|base| format!("{base}{}", integration.webhook_path));
}
