use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::admin_queries::{
        AccountPageResponse, AccountProjectionResponse, AdminPageQuery, CreateAccountRequest,
        CustomerPlanPageResponse,
    },
    dto::events::AccountEventResponse,
    error::{ApiResult, ErrorResponse},
    services::admin_queries,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_account))
        .routes(routes!(list_accounts))
        .routes(routes!(get_account))
        .routes(routes!(terminate_account))
        .routes(routes!(list_customer_plans))
}

#[utoipa::path(post, path = "/v1/admin/accounts", tag = "Operations", request_body = CreateAccountRequest,
    responses((status = 201, body = AccountProjectionResponse), (status = 422, body = ErrorResponse)))]
async fn create_account(
    State(state): State<AppState>,
    Json(request): Json<CreateAccountRequest>,
) -> ApiResult<(StatusCode, Json<AccountProjectionResponse>)> {
    let account = admin_queries::create_account(&state.database(), request).await?;
    Ok((StatusCode::CREATED, Json(account)))
}

#[utoipa::path(get, path = "/v1/admin/accounts", tag = "Operations", params(AdminPageQuery),
    responses((status = 200, body = AccountPageResponse), (status = 422, body = ErrorResponse)))]
async fn list_accounts(
    State(state): State<AppState>,
    Query(query): Query<AdminPageQuery>,
) -> ApiResult<Json<AccountPageResponse>> {
    Ok(Json(
        admin_queries::list_accounts(&state.database(), query).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/accounts/{account_id}", tag = "Operations", params(("account_id" = Uuid, Path)),
    responses((status = 200, body = AccountProjectionResponse), (status = 404, body = ErrorResponse)))]
async fn get_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AccountProjectionResponse>> {
    Ok(Json(
        admin_queries::get_account(&state.database(), id).await?,
    ))
}

#[utoipa::path(post, path = "/v1/admin/accounts/{account_id}/terminate", tag = "Operations",
    params(("account_id" = Uuid, Path)),
    responses((status = 200, body = AccountEventResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse)))]
async fn terminate_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AccountEventResponse>> {
    Ok(Json(
        admin_queries::terminate_account(&state.database(), id).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/accounts/{account_id}/customer-plans", tag = "Operations",
    params(("account_id" = Uuid, Path), AdminPageQuery),
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
