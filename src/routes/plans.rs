use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::plans::{
        CreateCustomerPlanRequest, CreateOnDemandPlanRequest, CreatePlanTransitionRequest,
        CreateSubscriptionPlanRequest, CreateSubscriptionRequest, CustomerPlanResponse,
        OnDemandPlanResponse, PlanTransitionResponse, RevokeCustomerPlanRequest, RevokePlanRequest,
        RunSubscriptionCyclesRequest, RunSubscriptionCyclesResponse, SubscriptionPlanResponse,
        SubscriptionResponse,
    },
    error::{ApiError, ApiResult, ErrorResponse},
    services::plans,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_subscription))
        .routes(routes!(get_subscription))
        .routes(routes!(create_subscription_plan))
        .routes(routes!(get_subscription_plan))
        .routes(routes!(revoke_subscription_plan))
        .routes(routes!(create_on_demand_plan))
        .routes(routes!(create_customer_plan))
        .routes(routes!(get_customer_plan))
        .routes(routes!(cancel_customer_plan))
        .routes(routes!(transition_customer_plan))
        .routes(routes!(revoke_customer_plan))
        .routes(routes!(run_subscription_cycles))
}

#[utoipa::path(post, path = "/v1/subscriptions", request_body = CreateSubscriptionRequest,
    responses((status = 201, body = SubscriptionResponse), (status = 422, body = ErrorResponse)))]
async fn create_subscription(
    State(state): State<AppState>,
    Json(request): Json<CreateSubscriptionRequest>,
) -> ApiResult<(StatusCode, Json<SubscriptionResponse>)> {
    Ok((
        StatusCode::CREATED,
        Json(plans::create_subscription(&state.database(), request).await?),
    ))
}

#[utoipa::path(get, path = "/v1/subscriptions/{subscription_id}",
    params(("subscription_id" = Uuid, Path)), responses((status = 200, body = SubscriptionResponse)))]
async fn get_subscription(
    State(state): State<AppState>,
    Path(subscription_id): Path<Uuid>,
) -> ApiResult<Json<SubscriptionResponse>> {
    Ok(Json(
        plans::get_subscription(&state.database(), subscription_id).await?,
    ))
}

#[utoipa::path(post, path = "/v1/subscriptions/{subscription_id}/plans",
    params(("subscription_id" = Uuid, Path)), request_body = CreateSubscriptionPlanRequest,
    responses((status = 201, body = SubscriptionPlanResponse), (status = 422, body = ErrorResponse)))]
async fn create_subscription_plan(
    State(state): State<AppState>,
    Path(subscription_id): Path<Uuid>,
    Json(request): Json<CreateSubscriptionPlanRequest>,
) -> ApiResult<(StatusCode, Json<SubscriptionPlanResponse>)> {
    Ok((
        StatusCode::CREATED,
        Json(plans::create_plan(&state.database(), subscription_id, request).await?),
    ))
}

#[utoipa::path(get, path = "/v1/subscription-plans/{plan_id}",
    params(("plan_id" = Uuid, Path)), responses((status = 200, body = SubscriptionPlanResponse)))]
async fn get_subscription_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<Uuid>,
) -> ApiResult<Json<SubscriptionPlanResponse>> {
    Ok(Json(plans::get_plan(&state.database(), plan_id).await?))
}

#[utoipa::path(post, path = "/v1/subscription-plans/{plan_id}/revoke",
    params(("plan_id" = Uuid, Path)), request_body = RevokePlanRequest,
    responses((status = 200, body = SubscriptionPlanResponse), (status = 409, body = ErrorResponse)))]
async fn revoke_subscription_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<Uuid>,
    Json(request): Json<RevokePlanRequest>,
) -> ApiResult<Json<SubscriptionPlanResponse>> {
    Ok(Json(
        plans::revoke_plan(&state.database(), plan_id, request).await?,
    ))
}

#[utoipa::path(post, path = "/v1/subscriptions/{subscription_id}/on-demand-plans",
    params(("subscription_id" = Uuid, Path)), request_body = CreateOnDemandPlanRequest,
    responses((status = 201, body = OnDemandPlanResponse), (status = 422, body = ErrorResponse)))]
async fn create_on_demand_plan(
    State(state): State<AppState>,
    Path(subscription_id): Path<Uuid>,
    Json(request): Json<CreateOnDemandPlanRequest>,
) -> ApiResult<(StatusCode, Json<OnDemandPlanResponse>)> {
    Ok((
        StatusCode::CREATED,
        Json(plans::create_on_demand_plan(&state.database(), subscription_id, request).await?),
    ))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/customer-plans",
    params(("workspace_id" = Uuid, Path), ("Idempotency-Key" = String, Header)),
    request_body = CreateCustomerPlanRequest,
    responses(
        (status = 201, body = CustomerPlanResponse),
        (status = 409, body = ErrorResponse, description = "Duplicate slot, key, transaction, or revoked plan"),
        (status = 503, body = ErrorResponse, description = "Wallet hierarchy is not ready")
    ))]
async fn create_customer_plan(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<CreateCustomerPlanRequest>,
) -> ApiResult<(StatusCode, Json<CustomerPlanResponse>)> {
    let key = idempotency_key(&headers)?;
    let response =
        plans::create_customer_plan(&state.database(), workspace_id, key, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}",
    params(("workspace_id" = Uuid, Path), ("customer_plan_id" = Uuid, Path)),
    responses((status = 200, body = CustomerPlanResponse)))]
async fn get_customer_plan(
    State(state): State<AppState>,
    Path((workspace_id, customer_plan_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<CustomerPlanResponse>> {
    Ok(Json(
        plans::get_customer_plan(&state.database(), workspace_id, customer_plan_id).await?,
    ))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/cancel",
    params(("workspace_id" = Uuid, Path), ("customer_plan_id" = Uuid, Path)),
    responses((status = 200, body = CustomerPlanResponse), (status = 409, body = ErrorResponse)))]
async fn cancel_customer_plan(
    State(state): State<AppState>,
    Path((workspace_id, customer_plan_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<CustomerPlanResponse>> {
    Ok(Json(
        plans::cancel_customer_plan(&state.database(), workspace_id, customer_plan_id).await?,
    ))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/plan-transitions",
    params(
        ("workspace_id" = Uuid, Path), ("customer_plan_id" = Uuid, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = CreatePlanTransitionRequest,
    responses(
        (status = 201, body = PlanTransitionResponse),
        (status = 409, body = ErrorResponse),
        (status = 503, body = ErrorResponse, description = "Paid transition requires Billing")
    ))]
async fn transition_customer_plan(
    State(state): State<AppState>,
    Path((workspace_id, customer_plan_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(request): Json<CreatePlanTransitionRequest>,
) -> ApiResult<(StatusCode, Json<PlanTransitionResponse>)> {
    let response = plans::transition_customer_plan(
        &state.database(),
        workspace_id,
        customer_plan_id,
        idempotency_key(&headers)?,
        request,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(post, path = "/v1/admin/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/revoke",
    params(("workspace_id" = Uuid, Path), ("customer_plan_id" = Uuid, Path)),
    request_body = RevokeCustomerPlanRequest,
    responses((status = 200, body = CustomerPlanResponse), (status = 409, body = ErrorResponse)))]
async fn revoke_customer_plan(
    State(state): State<AppState>,
    Path((workspace_id, customer_plan_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RevokeCustomerPlanRequest>,
) -> ApiResult<Json<CustomerPlanResponse>> {
    Ok(Json(
        plans::revoke_customer_plan(&state.database(), workspace_id, customer_plan_id, request)
            .await?,
    ))
}

#[utoipa::path(post, path = "/v1/admin/subscription-cycles/run",
    request_body = RunSubscriptionCyclesRequest,
    responses(
        (status = 200, body = RunSubscriptionCyclesResponse),
        (status = 503, body = ErrorResponse, description = "Cycle backlog exceeds one run")
    ))]
async fn run_subscription_cycles(
    State(state): State<AppState>,
    Json(request): Json<RunSubscriptionCyclesRequest>,
) -> ApiResult<Json<RunSubscriptionCyclesResponse>> {
    Ok(Json(
        plans::run_due_cycles(&state.database(), request.as_of).await?,
    ))
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
