use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::billing::{
        BillingCapabilitiesResponse, BillingConnectionResponse, BillingOperationsResponse,
        BillingWebhookResponse, CollectionRequestResponse, CreateBillingConnectionRequest,
        CreateInitialCollectionRequest, CreateOnDemandPurchaseRequest,
        CreatePaymentMethodBindingRequest, CreatePaymentMethodSetupSessionRequest,
        CreateRenewalRegularizationRequest, PaymentMethodBindingResponse,
        PaymentMethodSetupSessionResponse, UnmatchedPaymentCaseResponse,
    },
    error::{ApiError, ApiResult, ErrorResponse},
    services::billing,
    services::stripe_webhooks,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_billing_connection))
        .routes(routes!(get_billing_connection))
        .routes(routes!(get_billing_capabilities))
        .routes(routes!(create_payment_method_setup_session))
        .routes(routes!(receive_shared_stripe_webhook))
        .routes(routes!(create_payment_method_binding))
        .routes(routes!(list_payment_method_bindings))
        .routes(routes!(create_on_demand_purchase))
        .routes(routes!(create_initial_collection))
        .routes(routes!(get_collection_request))
        .routes(routes!(list_unmatched_payments))
        .routes(routes!(get_billing_operations))
        .routes(routes!(receive_stripe_webhook))
        .routes(routes!(create_renewal_regularization))
}

#[utoipa::path(get, path = "/v1/admin/workspaces/{workspace_id}/billing/unmatched-payments", tag = "Operations",
    params(("workspace_id" = Uuid, Path)), responses((status = 200, body = [UnmatchedPaymentCaseResponse])))]
async fn list_unmatched_payments(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<Vec<UnmatchedPaymentCaseResponse>>> {
    Ok(Json(
        billing::list_unmatched_payments(&state.database(), workspace_id).await?,
    ))
}

#[utoipa::path(get, path = "/v1/admin/billing/operations", tag = "Operations",
    responses((status = 200, body = BillingOperationsResponse)))]
async fn get_billing_operations(
    State(state): State<AppState>,
) -> ApiResult<Json<BillingOperationsResponse>> {
    Ok(Json(state.database().billing_operations().await?))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/collection-requests", tag = "Billing",
    params(("workspace_id" = Uuid, Path), ("customer_plan_id" = Uuid, Path), ("Idempotency-Key" = String, Header)),
    request_body = CreateInitialCollectionRequest,
    responses((status = 202, body = CollectionRequestResponse), (status = 409, body = ErrorResponse)))]
async fn create_initial_collection(
    State(state): State<AppState>,
    Path((workspace_id, customer_plan_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(request): Json<CreateInitialCollectionRequest>,
) -> ApiResult<(StatusCode, Json<CollectionRequestResponse>)> {
    let response = billing::create_initial_collection(
        &state.database(),
        workspace_id,
        customer_plan_id,
        idempotency_key(&headers)?,
        &request,
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(response)))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/collection-requests/{collection_request_id}", tag = "Billing",
    params(("workspace_id" = Uuid, Path), ("collection_request_id" = Uuid, Path)),
    responses((status = 200, body = CollectionRequestResponse), (status = 404, body = ErrorResponse)))]
async fn get_collection_request(
    State(state): State<AppState>,
    Path((workspace_id, collection_request_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<CollectionRequestResponse>> {
    Ok(Json(
        state
            .database()
            .find_collection_request(workspace_id, collection_request_id)
            .await?,
    ))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/on-demand-purchases", tag = "Billing",
    params(("workspace_id" = Uuid, Path), ("customer_plan_id" = Uuid, Path), ("Idempotency-Key" = String, Header)),
    request_body = CreateOnDemandPurchaseRequest,
    responses((status = 201, body = CollectionRequestResponse), (status = 409, body = ErrorResponse)))]
async fn create_on_demand_purchase(
    State(state): State<AppState>,
    Path((workspace_id, customer_plan_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(request): Json<CreateOnDemandPurchaseRequest>,
) -> ApiResult<(StatusCode, Json<CollectionRequestResponse>)> {
    let response = billing::create_on_demand_purchase(
        &state.database(),
        workspace_id,
        customer_plan_id,
        idempotency_key(&headers)?,
        &request,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(post, path = "/v1/billing/webhooks/{connection_id}", tag = "Billing",
    params(("connection_id" = Uuid, Path), ("Stripe-Signature" = String, Header)),
    request_body(content = String, content_type = "application/json"),
    responses((status = 200, body = BillingWebhookResponse), (status = 401, body = ErrorResponse)))]
async fn receive_stripe_webhook(
    State(state): State<AppState>,
    Path(connection_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<BillingWebhookResponse>> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::unauthorized(
                "stripe_signature_required",
                "Stripe-Signature header must contain visible ASCII",
            )
        })?;
    Ok(Json(
        stripe_webhooks::process_stripe_webhook(&state.database(), connection_id, signature, &body)
            .await?,
    ))
}

#[utoipa::path(
    post,
    path = "/v1/billing/webhooks/stripe",
    tag = "Billing",
    request_body(content = String, content_type = "application/json"),
    responses((status = 200, body = BillingWebhookResponse),
        (status = 401, body = ErrorResponse), (status = 409, body = ErrorResponse))
)]
async fn receive_shared_stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> ApiResult<Json<BillingWebhookResponse>> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::unauthorized(
                "stripe_signature_missing",
                "Stripe-Signature header is required",
            )
        })?;
    let config = state.billing_checkout_config().ok_or_else(|| {
        ApiError::service_unavailable(
            "billing_checkout_disabled",
            "shared Stripe webhook is not configured",
        )
    })?;
    let result = crate::services::stripe_webhooks::process_shared_stripe_webhook(
        &state.database(),
        signature,
        &body,
        &config.webhook_secret,
    )
    .await?;
    Ok(Json(result))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/billing-connections/{connection_id}/payment-method-setup-sessions", tag = "Billing",
    params(("workspace_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    request_body = CreatePaymentMethodSetupSessionRequest,
    responses((status = 201, body = PaymentMethodSetupSessionResponse), (status = 422, body = ErrorResponse)))]
async fn create_payment_method_setup_session(
    State(state): State<AppState>,
    Path((workspace_id, connection_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreatePaymentMethodSetupSessionRequest>,
) -> ApiResult<(StatusCode, Json<PaymentMethodSetupSessionResponse>)> {
    billing::get_billing_connection(&state.database(), workspace_id, connection_id).await?;
    let response =
        billing::create_payment_method_setup_session(&state.database(), connection_id, &request)
            .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/billing-connections", tag = "Billing",
    params(("workspace_id" = Uuid, Path)), request_body = CreateBillingConnectionRequest,
    responses((status = 201, body = BillingConnectionResponse), (status = 422, body = ErrorResponse)))]
async fn create_billing_connection(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<CreateBillingConnectionRequest>,
) -> ApiResult<(StatusCode, Json<BillingConnectionResponse>)> {
    let response =
        billing::create_billing_connection(&state.database(), workspace_id, &request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/billing-connections/{connection_id}", tag = "Billing",
    params(("workspace_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    responses((status = 200, body = BillingConnectionResponse), (status = 404, body = ErrorResponse)))]
async fn get_billing_connection(
    State(state): State<AppState>,
    Path((workspace_id, connection_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<BillingConnectionResponse>> {
    Ok(Json(
        billing::get_billing_connection(&state.database(), workspace_id, connection_id).await?,
    ))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/billing-connections/{connection_id}/capabilities", tag = "Billing",
    params(("workspace_id" = Uuid, Path), ("connection_id" = Uuid, Path)),
    responses((status = 200, body = BillingCapabilitiesResponse), (status = 404, body = ErrorResponse)))]
async fn get_billing_capabilities(
    State(state): State<AppState>,
    Path((workspace_id, connection_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<BillingCapabilitiesResponse>> {
    billing::get_billing_connection(&state.database(), workspace_id, connection_id).await?;
    Ok(Json(billing::billing_capabilities()))
}

#[utoipa::path(post, path = "/v1/workspaces/{workspace_id}/payment-method-bindings", tag = "Billing",
    params(("workspace_id" = Uuid, Path)), request_body = CreatePaymentMethodBindingRequest,
    responses((status = 201, body = PaymentMethodBindingResponse), (status = 409, body = ErrorResponse)))]
async fn create_payment_method_binding(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<CreatePaymentMethodBindingRequest>,
) -> ApiResult<(StatusCode, Json<PaymentMethodBindingResponse>)> {
    let response =
        billing::create_payment_method_binding(&state.database(), workspace_id, &request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(get, path = "/v1/workspaces/{workspace_id}/payment-method-bindings", tag = "Billing",
    params(("workspace_id" = Uuid, Path)), responses((status = 200, body = [PaymentMethodBindingResponse])))]
async fn list_payment_method_bindings(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<Vec<PaymentMethodBindingResponse>>> {
    Ok(Json(
        billing::list_payment_method_bindings(&state.database(), workspace_id).await?,
    ))
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
