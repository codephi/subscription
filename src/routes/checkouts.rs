use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::checkouts::{
        CheckoutQuoteRequest, CheckoutQuoteResponse, CheckoutResponse, CreateCheckoutRequest,
    },
    error::{ApiError, ApiResult, ErrorResponse},
    services::billing_checkout,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_checkout))
        .routes(routes!(quote_checkout))
        .routes(routes!(get_checkout))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/checkout-quotes", tag = "Checkouts",
    params(("workspace_id" = Uuid, Path)), request_body = CheckoutQuoteRequest,
    responses((status = 200, body = CheckoutQuoteResponse), (status = 409, body = ErrorResponse)))]
async fn quote_checkout(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<CheckoutQuoteRequest>,
) -> ApiResult<Json<CheckoutQuoteResponse>> {
    Ok(Json(
        billing_checkout::quote(&state.database(), workspace_id, request).await?,
    ))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/checkouts", tag = "Checkouts",
    params(("workspace_id" = Uuid, Path), ("Idempotency-Key" = String, Header)),
    request_body = CreateCheckoutRequest,
    responses((status = 201, body = CheckoutResponse), (status = 202, body = CheckoutResponse),
        (status = 409, body = ErrorResponse), (status = 503, body = ErrorResponse)))]
async fn create_checkout(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<CreateCheckoutRequest>,
) -> ApiResult<(StatusCode, Json<CheckoutResponse>)> {
    let response = billing_checkout::create(
        &state.database(),
        state.billing_checkout_config().as_ref(),
        workspace_id,
        idempotency_key(&headers)?,
        request,
    )
    .await?;
    let status = if response.collection_request_id.is_some() {
        StatusCode::CREATED
    } else {
        StatusCode::ACCEPTED
    };
    Ok((status, Json(response)))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/checkouts/{checkout_id}", tag = "Checkouts",
    params(("workspace_id" = Uuid, Path), ("checkout_id" = Uuid, Path)),
    responses((status = 200, body = CheckoutResponse), (status = 404, body = ErrorResponse)))]
async fn get_checkout(
    State(state): State<AppState>,
    Path((workspace_id, checkout_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<CheckoutResponse>> {
    Ok(Json(
        billing_checkout::get(&state.database(), workspace_id, checkout_id).await?,
    ))
}

fn idempotency_key(headers: &HeaderMap) -> ApiResult<&str> {
    let value = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 255 && value.is_ascii());
    value.ok_or_else(|| {
        ApiError::unprocessable(
            "idempotency_key_required",
            "Idempotency-Key must contain 1 to 255 ASCII characters",
        )
    })
}
