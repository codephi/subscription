use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::promotions::{
        CreateCouponRequest, CreateVoucherRequest, PromotionHistoryResponse, PromotionListQuery,
        PromotionPageResponse, PromotionResponse, RedeemVoucherRequest, UpdatePromotionRequest,
        VoucherRedemptionResponse,
    },
    error::{ApiError, ApiResult, ErrorResponse},
    services::promotions,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_voucher, list_vouchers))
        .routes(routes!(get_voucher, update_voucher))
        .routes(routes!(voucher_history))
        .routes(routes!(create_coupon, list_coupons))
        .routes(routes!(get_coupon, update_coupon))
        .routes(routes!(coupon_history))
        .routes(routes!(redeem_voucher))
}

#[utoipa::path(post, path = "/v1/admin/vouchers", tag = "Promotions", request_body = CreateVoucherRequest, responses((status = 201, body = PromotionResponse), (status = 422, body = ErrorResponse)))]
async fn create_voucher(
    State(state): State<AppState>,
    Json(body): Json<CreateVoucherRequest>,
) -> ApiResult<(StatusCode, Json<PromotionResponse>)> {
    Ok((
        StatusCode::CREATED,
        Json(promotions::create_voucher(&state.database(), body).await?),
    ))
}

#[utoipa::path(get, path = "/v1/admin/vouchers", tag = "Promotions", params(PromotionListQuery), responses((status = 200, body = PromotionPageResponse)))]
async fn list_vouchers(
    State(state): State<AppState>,
    Query(query): Query<PromotionListQuery>,
) -> ApiResult<Json<PromotionPageResponse>> {
    Ok(Json(
        promotions::list(&state.database(), "VOUCHER", query).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/vouchers/{id}", tag = "Promotions", params(("id" = Uuid, Path)), responses((status = 200, body = PromotionResponse)))]
async fn get_voucher(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PromotionResponse>> {
    Ok(Json(state.database().get_promotion("VOUCHER", id).await?))
}

#[utoipa::path(patch, path = "/v1/admin/vouchers/{id}", tag = "Promotions", params(("id" = Uuid, Path)), request_body = UpdatePromotionRequest, responses((status = 200, body = PromotionResponse), (status = 409, body = ErrorResponse)))]
async fn update_voucher(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePromotionRequest>,
) -> ApiResult<Json<PromotionResponse>> {
    Ok(Json(
        promotions::update(&state.database(), "VOUCHER", id, body).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/vouchers/{id}/history", tag = "Promotions", params(("id" = Uuid, Path)), responses((status = 200, body = PromotionHistoryResponse)))]
async fn voucher_history(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PromotionHistoryResponse>> {
    Ok(Json(
        state.database().promotion_history("VOUCHER", id).await?,
    ))
}

#[utoipa::path(post, path = "/v1/admin/coupons", tag = "Promotions", request_body = CreateCouponRequest, responses((status = 201, body = PromotionResponse), (status = 422, body = ErrorResponse)))]
async fn create_coupon(
    State(state): State<AppState>,
    Json(body): Json<CreateCouponRequest>,
) -> ApiResult<(StatusCode, Json<PromotionResponse>)> {
    Ok((
        StatusCode::CREATED,
        Json(promotions::create_coupon(&state.database(), body).await?),
    ))
}

#[utoipa::path(get, path = "/v1/admin/coupons", tag = "Promotions", params(PromotionListQuery), responses((status = 200, body = PromotionPageResponse)))]
async fn list_coupons(
    State(state): State<AppState>,
    Query(query): Query<PromotionListQuery>,
) -> ApiResult<Json<PromotionPageResponse>> {
    Ok(Json(
        promotions::list(&state.database(), "COUPON", query).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/coupons/{id}", tag = "Promotions", params(("id" = Uuid, Path)), responses((status = 200, body = PromotionResponse)))]
async fn get_coupon(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PromotionResponse>> {
    Ok(Json(state.database().get_promotion("COUPON", id).await?))
}

#[utoipa::path(patch, path = "/v1/admin/coupons/{id}", tag = "Promotions", params(("id" = Uuid, Path)), request_body = UpdatePromotionRequest, responses((status = 200, body = PromotionResponse), (status = 409, body = ErrorResponse)))]
async fn update_coupon(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePromotionRequest>,
) -> ApiResult<Json<PromotionResponse>> {
    Ok(Json(
        promotions::update(&state.database(), "COUPON", id, body).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/coupons/{id}/history", tag = "Promotions", params(("id" = Uuid, Path)), responses((status = 200, body = PromotionHistoryResponse)))]
async fn coupon_history(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PromotionHistoryResponse>> {
    Ok(Json(
        state.database().promotion_history("COUPON", id).await?,
    ))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/voucher-redemptions", tag = "Promotions", params(("workspace_id" = Uuid, Path), ("Idempotency-Key" = String, Header)), request_body = RedeemVoucherRequest, responses((status = 201, body = VoucherRedemptionResponse), (status = 409, body = ErrorResponse)))]
async fn redeem_voucher(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<RedeemVoucherRequest>,
) -> ApiResult<(StatusCode, Json<VoucherRedemptionResponse>)> {
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::unprocessable(
                "idempotency_key_required",
                "Idempotency-Key must contain 1 to 255 ASCII characters",
            )
        })?;
    let response = promotions::redeem_voucher(&state.database(), workspace_id, key, body).await?;
    Ok((StatusCode::CREATED, Json(response)))
}
