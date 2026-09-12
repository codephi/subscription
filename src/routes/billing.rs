use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::billing::{CollectionRequestResponse, CreateRenewalRegularizationRequest},
    error::{ApiError, ApiResult, ErrorResponse},
    services::billing,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(create_renewal_regularization))
}

#[utoipa::path(
    post,
    path = "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/renewal-regularizations",
    tag = "Billing",
    params(
        ("workspace_id" = Uuid, Path),
        ("customer_plan_id" = Uuid, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = CreateRenewalRegularizationRequest,
    responses(
        (status = 201, body = CollectionRequestResponse),
        (status = 409, body = ErrorResponse, description = "Plan is not eligible or the key has different parameters"),
        (status = 422, body = ErrorResponse, description = "Idempotency or transaction identifier is invalid"),
        (status = 503, body = ErrorResponse, description = "Wallet hierarchy is not ready")
    )
)]
async fn create_renewal_regularization(
    State(state): State<AppState>,
    Path((workspace_id, customer_plan_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(request): Json<CreateRenewalRegularizationRequest>,
) -> ApiResult<(StatusCode, Json<CollectionRequestResponse>)> {
    let response = billing::create_renewal_regularization(
        &state.database(),
        workspace_id,
        customer_plan_id,
        idempotency_key(&headers)?,
        &request,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

fn idempotency_key(headers: &HeaderMap) -> ApiResult<&str> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::unprocessable(
                "idempotency_key_required",
                "Idempotency-Key header must contain visible ASCII",
            )
        })
}
