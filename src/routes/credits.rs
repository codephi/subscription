use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::credits::{
        CreditLedgerReconciliationResponse, CustomerWalletEntryResponse,
        CustomerWalletStatementResponse, DirectCreditRequest, DirectCreditResponse, StatementQuery,
        UpdateWorkspaceBillingConfigRequest, WorkspaceBillingConfigResponse,
    },
    error::{ApiError, ApiResult, ErrorResponse},
    services::credits,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(grant_direct_credit))
        .routes(routes!(customer_wallet_statement))
        .routes(routes!(find_customer_wallet_transaction))
        .routes(routes!(reconcile_credit_ledger))
        .routes(routes!(get_billing_config, update_billing_config))
}

#[utoipa::path(
    post,
    path = "/v1/workspaces/{workspace_id}/credits/direct",
    params(
        ("workspace_id" = Uuid, Path),
        ("Idempotency-Key" = String, Header, description = "Strict single-use idempotency key")
    ),
    request_body = DirectCreditRequest,
    responses(
        (status = 201, body = DirectCreditResponse),
        (status = 409, body = ErrorResponse, description = "Duplicate key, transaction, or inactive workspace"),
        (status = 422, body = ErrorResponse, description = "Invalid units or context"),
        (status = 503, body = ErrorResponse, description = "Wallet hierarchy is incomplete")
    )
)]
async fn grant_direct_credit(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<DirectCreditRequest>,
) -> ApiResult<(StatusCode, Json<DirectCreditResponse>)> {
    let key = required_idempotency_key(&headers)?;
    let response =
        credits::grant_direct_credit(&state.database(), workspace_id, key, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    get,
    path = "/v1/workspaces/{workspace_id}/customer-wallet/statement",
    params(("workspace_id" = Uuid, Path), StatementQuery),
    responses(
        (status = 200, body = CustomerWalletStatementResponse),
        (status = 422, body = ErrorResponse, description = "Invalid cursor or page limit")
    )
)]
async fn customer_wallet_statement(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<StatementQuery>,
) -> ApiResult<Json<CustomerWalletStatementResponse>> {
    Ok(Json(
        credits::statement(&state.database(), workspace_id, query).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/workspaces/{workspace_id}/customer-wallet/transactions/{transaction_id}",
    params(("workspace_id" = Uuid, Path), ("transaction_id" = String, Path)),
    responses(
        (status = 200, body = CustomerWalletEntryResponse),
        (status = 404, body = ErrorResponse, description = "Transaction does not exist")
    )
)]
async fn find_customer_wallet_transaction(
    State(state): State<AppState>,
    Path((workspace_id, transaction_id)): Path<(Uuid, String)>,
) -> ApiResult<Json<CustomerWalletEntryResponse>> {
    Ok(Json(
        credits::find_transaction(&state.database(), workspace_id, &transaction_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/v1/admin/workspaces/{workspace_id}/customer-wallet/reconcile",
    params(("workspace_id" = Uuid, Path)),
    responses(
        (status = 200, body = CreditLedgerReconciliationResponse),
        (status = 503, body = ErrorResponse, description = "Customer wallet is absent")
    )
)]
async fn reconcile_credit_ledger(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<CreditLedgerReconciliationResponse>> {
    Ok(Json(
        credits::reconcile(&state.database(), workspace_id).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/workspaces/{workspace_id}/billing-config",
    params(("workspace_id" = Uuid, Path)),
    responses((status = 200, body = WorkspaceBillingConfigResponse))
)]
async fn get_billing_config(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<WorkspaceBillingConfigResponse>> {
    Ok(Json(
        credits::get_billing_config(&state.database(), workspace_id).await?,
    ))
}

#[utoipa::path(
    put,
    path = "/v1/workspaces/{workspace_id}/billing-config",
    params(("workspace_id" = Uuid, Path)),
    request_body = UpdateWorkspaceBillingConfigRequest,
    responses(
        (status = 200, body = WorkspaceBillingConfigResponse),
        (status = 409, body = ErrorResponse, description = "Optimistic version conflict")
    )
)]
async fn update_billing_config(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<UpdateWorkspaceBillingConfigRequest>,
) -> ApiResult<Json<WorkspaceBillingConfigResponse>> {
    Ok(Json(
        credits::update_billing_config(&state.database(), workspace_id, request).await?,
    ))
}

fn required_idempotency_key(headers: &HeaderMap) -> ApiResult<&str> {
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
