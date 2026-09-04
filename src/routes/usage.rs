use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::usage::{
        CreateUsageEventRequest, ItemStatementQuery, ItemWalletEntryResponse,
        ItemWalletMeterResponse, ItemWalletStatementResponse, PricingAccumulatorQuery,
        PricingAccumulatorResponse, ProductEligibilityResponse, UsageEventResponse,
        UsageReconciliationResponse,
    },
    error::{ApiError, ApiResult, ErrorResponse},
    services::usage,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_usage_event))
        .routes(routes!(get_product_eligibility))
        .routes(routes!(get_item_meter))
        .routes(routes!(get_item_statement))
        .routes(routes!(get_item_statement_entry))
        .routes(routes!(get_pricing_accumulators))
        .routes(routes!(reconcile_item_usage))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/usage-events", tag = "Usage",
    params(("workspace_id" = Uuid, Path), ("Idempotency-Key" = String, Header)),
    request_body = CreateUsageEventRequest,
    responses(
        (status = 201, body = UsageEventResponse),
        (status = 403, body = ErrorResponse, description = "Product entitlement is absent"),
        (status = 409, body = ErrorResponse, description = "Price changed or credit is insufficient"),
        (status = 503, body = ErrorResponse, description = "Wallet hierarchy is not ready")
    ))]
async fn create_usage_event(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<CreateUsageEventRequest>,
) -> ApiResult<(StatusCode, Json<UsageEventResponse>)> {
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::unprocessable(
                "idempotency_key_required",
                "Idempotency-Key header must contain visible ASCII",
            )
        })?;
    let response = usage::record_usage(&state.database(), workspace_id, key, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/products/{product_id}/eligibility", tag = "Usage",
    params(("workspace_id" = Uuid, Path), ("product_id" = Uuid, Path)),
    responses((status = 200, body = ProductEligibilityResponse), (status = 404, body = ErrorResponse)))]
async fn get_product_eligibility(
    State(state): State<AppState>,
    Path((workspace_id, product_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ProductEligibilityResponse>> {
    Ok(Json(
        usage::eligibility(&state.database(), workspace_id, product_id).await?,
    ))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet", tag = "Usage",
    params(("workspace_id" = Uuid, Path), ("item_id" = Uuid, Path)),
    responses((status = 200, body = ItemWalletMeterResponse), (status = 503, body = ErrorResponse)))]
async fn get_item_meter(
    State(state): State<AppState>,
    Path((workspace_id, item_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ItemWalletMeterResponse>> {
    Ok(Json(
        usage::item_meter(&state.database(), workspace_id, item_id).await?,
    ))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement", tag = "Usage",
    params(("workspace_id" = Uuid, Path), ("item_id" = Uuid, Path), ItemStatementQuery),
    responses((status = 200, body = ItemWalletStatementResponse), (status = 422, body = ErrorResponse)))]
async fn get_item_statement(
    State(state): State<AppState>,
    Path((workspace_id, item_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ItemStatementQuery>,
) -> ApiResult<Json<ItemWalletStatementResponse>> {
    Ok(Json(
        usage::item_statement(&state.database(), workspace_id, item_id, query).await?,
    ))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement/{entry_id}", tag = "Usage",
    params(("workspace_id" = Uuid, Path), ("item_id" = Uuid, Path), ("entry_id" = Uuid, Path)),
    responses((status = 200, body = ItemWalletEntryResponse), (status = 404, body = ErrorResponse)))]
async fn get_item_statement_entry(
    State(state): State<AppState>,
    Path((workspace_id, item_id, entry_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<ItemWalletEntryResponse>> {
    Ok(Json(
        usage::item_statement_entry(&state.database(), workspace_id, item_id, entry_id).await?,
    ))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/pricing-accumulators", tag = "Usage",
    params(("workspace_id" = Uuid, Path), ("item_id" = Uuid, Path), PricingAccumulatorQuery),
    responses((status = 200, body = [PricingAccumulatorResponse])))]
async fn get_pricing_accumulators(
    State(state): State<AppState>,
    Path((workspace_id, item_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<PricingAccumulatorQuery>,
) -> ApiResult<Json<Vec<PricingAccumulatorResponse>>> {
    Ok(Json(
        usage::pricing_accumulators(
            &state.database(),
            workspace_id,
            item_id,
            query.price_version_id,
        )
        .await?,
    ))
}

#[utoipa::path(post, path = "/v1/admin/workspaces/{workspace_id}/items/{item_id}/usage/reconcile", tag = "Operations",
    params(("workspace_id" = Uuid, Path), ("item_id" = Uuid, Path)),
    responses((status = 200, body = UsageReconciliationResponse), (status = 503, body = ErrorResponse)))]
async fn reconcile_item_usage(
    State(state): State<AppState>,
    Path((workspace_id, item_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<UsageReconciliationResponse>> {
    Ok(Json(
        usage::reconcile_item(&state.database(), workspace_id, item_id).await?,
    ))
}
