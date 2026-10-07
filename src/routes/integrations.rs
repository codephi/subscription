use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::billing::{
        AccountIntegrationResponse, CreateStripeIntegrationRequest,
        DefaultStripeCredentialsResponse, IntegrationProviderResponse,
        StripeIntegrationTestResponse, UpdateDefaultStripeCredentialsRequest,
        UpdateStripeIntegrationRequest,
    },
    error::{ApiResult, ErrorResponse},
    services::billing_integrations as integrations,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_providers))
        .routes(routes!(get_default_stripe_credentials))
        .routes(routes!(update_default_stripe_credentials))
        .routes(routes!(list_account_integrations))
        .routes(routes!(get_account_integration))
        .routes(routes!(create_account_integration))
        .routes(routes!(update_account_integration))
        .routes(routes!(test_account_integration))
}

#[utoipa::path(get, path = "/v1/admin/billing/stripe-defaults", tag = "Operations",
    responses((status = 200, body = DefaultStripeCredentialsResponse)))]
async fn get_default_stripe_credentials(
    State(state): State<AppState>,
) -> ApiResult<Json<DefaultStripeCredentialsResponse>> {
    Ok(Json(
        integrations::default_stripe_credentials(&state.database()).await?,
    ))
}

#[utoipa::path(put, path = "/v1/admin/billing/stripe-defaults", tag = "Operations",
    request_body = UpdateDefaultStripeCredentialsRequest,
    responses((status = 200, body = DefaultStripeCredentialsResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse)))]
async fn update_default_stripe_credentials(
    State(state): State<AppState>,
    Json(request): Json<UpdateDefaultStripeCredentialsRequest>,
) -> ApiResult<Json<DefaultStripeCredentialsResponse>> {
    Ok(Json(
        integrations::update_default_stripe_credentials(&state.database(), request).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/integrations/providers", tag = "Operations",
    responses((status = 200, body = [IntegrationProviderResponse])))]
async fn list_providers() -> Json<Vec<IntegrationProviderResponse>> {
    Json(integrations::providers())
}

#[utoipa::path(get, path = "/v1/admin/accounts/{account_id}/integrations", tag = "Operations",
    params(("account_id" = Uuid, Path)),
    responses((status = 200, body = [AccountIntegrationResponse])))]
async fn list_account_integrations(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
) -> ApiResult<Json<Vec<AccountIntegrationResponse>>> {
    let mut result = integrations::list(&state.database(), account_id).await?;
    result
        .iter_mut()
        .for_each(|item| attach_webhook_url(&state, item));
    Ok(Json(result))
}

#[utoipa::path(get, path = "/v1/admin/accounts/{account_id}/integrations/{connection_id}", tag = "Operations",
    params(("account_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    responses((status = 200, body = AccountIntegrationResponse), (status = 404, body = ErrorResponse)))]
async fn get_account_integration(
    State(state): State<AppState>,
    Path((account_id, connection_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<AccountIntegrationResponse>> {
    let mut result = integrations::get(&state.database(), account_id, connection_id).await?;
    attach_webhook_url(&state, &mut result);
    Ok(Json(result))
}

#[utoipa::path(post, path = "/v1/admin/accounts/{account_id}/integrations", tag = "Operations",
    params(("account_id" = Uuid, Path)), request_body = CreateStripeIntegrationRequest,
    responses((status = 201, body = AccountIntegrationResponse), (status = 422, body = ErrorResponse)))]
async fn create_account_integration(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
    Json(request): Json<CreateStripeIntegrationRequest>,
) -> ApiResult<(StatusCode, Json<AccountIntegrationResponse>)> {
    let mut result = integrations::create_stripe(&state.database(), account_id, &request).await?;
    attach_webhook_url(&state, &mut result);
    Ok((StatusCode::CREATED, Json(result)))
}

#[utoipa::path(patch, path = "/v1/admin/accounts/{account_id}/integrations/{connection_id}", tag = "Operations",
    params(("account_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    request_body = UpdateStripeIntegrationRequest,
    responses((status = 200, body = AccountIntegrationResponse), (status = 409, body = ErrorResponse)))]
async fn update_account_integration(
    State(state): State<AppState>,
    Path((account_id, connection_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateStripeIntegrationRequest>,
) -> ApiResult<Json<AccountIntegrationResponse>> {
    let mut result =
        integrations::update_stripe(&state.database(), account_id, connection_id, &request).await?;
    attach_webhook_url(&state, &mut result);
    Ok(Json(result))
}

#[utoipa::path(post, path = "/v1/admin/accounts/{account_id}/integrations/{connection_id}/test", tag = "Operations",
    params(("account_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    responses((status = 200, body = StripeIntegrationTestResponse), (status = 422, body = ErrorResponse)))]
async fn test_account_integration(
    State(state): State<AppState>,
    Path((account_id, connection_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<StripeIntegrationTestResponse>> {
    Ok(Json(
        integrations::test_stripe(&state.database(), account_id, connection_id).await?,
    ))
}

fn attach_webhook_url(state: &AppState, integration: &mut AccountIntegrationResponse) {
    integration.webhook_url = state
        .public_api_base_url()
        .map(|base| format!("{base}{}", integration.webhook_path));
}
