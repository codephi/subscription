use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::credits::{
        AccountBillingConfigResponse, AccountTransactionResponse,
        CreditLedgerReconciliationResponse, CustomerWalletStatementResponse, DirectCreditRequest,
        DirectCreditResponse, StatementQuery, UpdateAccountBillingConfigRequest,
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
    path = "/v1/accounts/{account_id}/credits/direct",
    tag = "Credits",
    params(
        ("account_id" = Uuid, Path),
        ("Idempotency-Key" = String, Header, description = "Strict single-use idempotency key")
    ),
    request_body = DirectCreditRequest,
    responses(
        (status = 201, body = DirectCreditResponse),
        (status = 409, body = ErrorResponse, description = "Duplicate key or transaction includes existing_operation when its committed reference is available; inactive accounts and disabled grants also conflict"),
        (status = 422, body = ErrorResponse, description = "Invalid units or context"),
        (status = 503, body = ErrorResponse, description = "Wallet hierarchy is incomplete")
    )
)]
async fn grant_direct_credit(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<DirectCreditRequest>,
) -> ApiResult<(StatusCode, Json<DirectCreditResponse>)> {
    let key = required_idempotency_key(&headers)?;
    let response =
        credits::grant_direct_credit(&state.database(), account_id, key, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    get,
    path = "/v1/accounts/{account_id}/customer-wallet/statement",
    tag = "Credits",
    params(("account_id" = Uuid, Path), StatementQuery),
    responses(
        (status = 200, body = CustomerWalletStatementResponse),
        (status = 422, body = ErrorResponse, description = "Invalid cursor or page limit")
    )
)]
async fn customer_wallet_statement(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
    Query(query): Query<StatementQuery>,
) -> ApiResult<Json<CustomerWalletStatementResponse>> {
    Ok(Json(
        credits::statement(&state.database(), account_id, query).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/accounts/{account_id}/customer-wallet/transactions/{transaction_id}",
    tag = "Credits",
    params(("account_id" = Uuid, Path), ("transaction_id" = String, Path)),
    responses(
        (status = 200, body = AccountTransactionResponse),
        (status = 404, body = ErrorResponse, description = "Transaction does not exist")
    )
)]
async fn find_customer_wallet_transaction(
    State(state): State<AppState>,
    Path((account_id, transaction_id)): Path<(Uuid, String)>,
) -> ApiResult<Json<AccountTransactionResponse>> {
    Ok(Json(
        credits::find_transaction(&state.database(), account_id, &transaction_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/v1/admin/accounts/{account_id}/customer-wallet/reconcile",
    tag = "Operations",
    params(("account_id" = Uuid, Path)),
    responses(
        (status = 200, body = CreditLedgerReconciliationResponse, description = "Read-only comparison of wallet balance, summed ledger with continuous sequence/balance chain, and unexpired remaining lots; divergence never repairs history"),
        (status = 503, body = ErrorResponse, description = "Customer wallet is absent")
    )
)]
async fn reconcile_credit_ledger(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
) -> ApiResult<Json<CreditLedgerReconciliationResponse>> {
    Ok(Json(
        credits::reconcile(&state.database(), account_id).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/accounts/{account_id}/billing-config",
    tag = "Credits",
    params(("account_id" = Uuid, Path)),
    responses((status = 200, body = AccountBillingConfigResponse))
)]
async fn get_billing_config(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
) -> ApiResult<Json<AccountBillingConfigResponse>> {
    Ok(Json(
        credits::get_billing_config(&state.database(), account_id).await?,
    ))
}

#[utoipa::path(
    put,
    path = "/v1/accounts/{account_id}/billing-config",
    tag = "Credits",
    params(("account_id" = Uuid, Path)),
    request_body = UpdateAccountBillingConfigRequest,
    responses(
        (status = 200, body = AccountBillingConfigResponse),
        (status = 409, body = ErrorResponse, description = "Optimistic version conflict")
    )
)]
async fn update_billing_config(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
    Json(request): Json<UpdateAccountBillingConfigRequest>,
) -> ApiResult<Json<AccountBillingConfigResponse>> {
    Ok(Json(
        credits::update_billing_config(&state.database(), account_id, request).await?,
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
