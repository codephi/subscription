use axum::{
    extract::{Path, Query, State},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::billing_investigation::{
        BillingRecordPageResponse, BillingRecordQuery, BillingRecordResponse,
    },
    error::{ApiResult, ErrorResponse},
    services::billing_investigation,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_records))
        .routes(routes!(get_record))
}

#[utoipa::path(get, path = "/v1/admin/billing/records/{kind}", tag = "Operations",
    params(("kind" = String, Path, description = "collections, attempts, payments, webhooks, outbox, unmatched"), BillingRecordQuery),
    responses((status = 200, body = BillingRecordPageResponse), (status = 422, body = ErrorResponse)))]
async fn list_records(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    Query(query): Query<BillingRecordQuery>,
) -> ApiResult<Json<BillingRecordPageResponse>> {
    Ok(Json(
        billing_investigation::list_records(&state.database(), &kind, query).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/billing/records/{kind}/{id}", tag = "Operations",
    params(("kind" = String, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = BillingRecordResponse), (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse)))]
async fn get_record(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> ApiResult<Json<BillingRecordResponse>> {
    Ok(Json(
        billing_investigation::get_record(&state.database(), &kind, id).await?,
    ))
}
