use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    routing::{get, patch, post},
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::{FromRow, Row};
use uuid::Uuid;

use crate::{auth, errors::AppError, models::*, services, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/plan", post(choose_plan))
        .route("/api/plan/cancel", post(cancel_plan))
        .route("/api/plan/regularize", post(regularize_plan))
        .route("/api/dashboard", get(dashboard))
        .route("/api/checkouts", post(checkout))
        .route("/api/checkouts/{id}", get(checkout_status))
        .route("/api/payment-methods", get(payment_methods))
        .route("/api/payment-method-sessions", post(payment_method_session))
        .route("/api/subscription-payment-method-session", post(subscription_payment_method_session))
        .route("/api/payment-method-bindings", post(confirm_payment_method))
        .route("/api/payment-methods/{id}", patch(rename_payment_method).delete(remove_payment_method))
        .route("/api/executions", post(execute))
        .route("/api/history", get(history))
}

async fn register(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, HeaderValue); 1],
        Json<AccountResponse>,
    ),
    AppError,
> {
    let user = auth::register(&state.pool, &request.username, &request.password).await?;
    let user = services::provision_account(&state, &user).await?;
    let cookie = auth::create_session(&state.pool, &user).await?;
    Ok((
        StatusCode::CREATED,
        [(header::SET_COOKIE, cookie)],
        Json(auth::account_response(user)?),
    ))
}

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<
    (
        [(header::HeaderName, HeaderValue); 1],
        Json<AccountResponse>,
    ),
    AppError,
> {
    let user = auth::authenticate(&state.pool, &request.username, &request.password).await?;
    let user = services::provision_account(&state, &user).await?;
    let cookie = auth::create_session(&state.pool, &user).await?;
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(auth::account_response(user)?),
    ))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<([(header::HeaderName, HeaderValue); 1], StatusCode), AppError> {
    let cookie = auth::delete_session(&state.pool, &headers).await?;
    Ok(([(header::SET_COOKIE, cookie)], StatusCode::NO_CONTENT))
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AccountResponse>, AppError> {
    Ok(Json(auth::account_response(
        auth::current_user(&state, &headers).await?,
    )?))
}

async fn choose_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChoosePlanRequest>,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    Ok(Json(
        services::choose_plan(&state, &user, request.plan_model).await?,
    ))
}

async fn cancel_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    Ok(Json(services::cancel_plan(&state, &user).await?))
}

async fn regularize_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateRegularizationRequest>,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    Ok(Json(
        services::regularize_plan(&state, &user, &request.transaction_id).await?,
    ))
}

async fn checkout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateCheckoutRequest>,
) -> Result<(StatusCode, Json<CheckoutResponse>), AppError> {
    let user = auth::current_user(&state, &headers).await?;
    let transaction = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 128 && value.is_ascii())
        .ok_or_else(|| {
            AppError::Invalid("Idempotency-Key deve conter de 1 a 128 caracteres ASCII".into())
        })?
        .to_owned();
    let response = services::create_checkout(
        &state,
        &user,
        request.checkout_kind,
        request.topup_credits,
        request.payment_method_binding_id,
        request.target_plan_version_id,
        request.save_payment_method,
        request.payment_method_name,
        transaction,
        headers
            .get(header::ORIGIN)
            .and_then(|origin| origin.to_str().ok()),
    )
    .await?;
    let status = if response.status == "PENDING" {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(response)))
}

async fn checkout_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<CheckoutResponse>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    Ok(Json(services::refresh_checkout(&state, &user, id).await?))
}

async fn payment_methods(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    let body = state.subscription.get(&format!("/v1/accounts/{}/payment-method-bindings", user.account_id)).await?;
    Ok(Json(body))
}

async fn payment_method_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PaymentMethodSetupRequest>,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    let customer_plan_id = user.customer_plan_id.as_deref().ok_or_else(|| AppError::Conflict("ative um plano antes de cadastrar um cartão".into()))?;
    let (success_url, cancel_url) = services::checkout_return_urls(&state.app_public_url, headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()))?;
    let body = state.subscription.post::<Value>(
        &format!("/v1/accounts/{}/payment-method-setup-sessions", user.account_id), None,
        &json!({"customer_plan_id":customer_plan_id,"success_url":success_url,"cancel_url":cancel_url,"card_name":request.card_name}),
    ).await?;
    Ok(Json(body))
}

async fn subscription_payment_method_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    let customer_plan_id = user.customer_plan_id.as_deref().ok_or_else(|| AppError::Conflict("ative um plano antes de atualizar o cartão de renovação".into()))?;
    let customer_plan_id = Uuid::parse_str(customer_plan_id).map_err(|error| AppError::Internal(error.into()))?;
    let (return_url, _) = services::checkout_return_urls(&state.app_public_url, headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()))?;
    let body = state.subscription.post::<Value>(
        &format!("/v1/accounts/{}/customer-plans/{customer_plan_id}/payment-method-management-sessions", user.account_id), None,
        &json!({"return_url":return_url}),
    ).await?;
    Ok(Json(body))
}

async fn confirm_payment_method(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ConfirmPaymentMethodRequest>,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    let customer_plan_id = user.customer_plan_id.as_deref().ok_or_else(|| AppError::Conflict("ative um plano antes de cadastrar um cartão".into()))?;
    let body = state.subscription.post::<Value>(
        &format!("/v1/accounts/{}/payment-method-bindings", user.account_id), None,
        &json!({"customer_plan_id":customer_plan_id,"payment_method_setup_id":request.payment_method_setup_id,"card_name":request.card_name}),
    ).await?;
    Ok(Json(body))
}

async fn rename_payment_method(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<RenamePaymentMethodRequest>,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    let body = state.subscription.patch::<Value>(
        &format!("/v1/accounts/{}/payment-method-bindings/{id}", user.account_id),
        &json!({"display_name":request.display_name}),
    ).await?;
    Ok(Json(body))
}

async fn remove_payment_method(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    state.subscription.delete(&format!("/v1/accounts/{}/payment-method-bindings/{id}", user.account_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn execute(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateExecutionRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let user = auth::current_user(&state, &headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            services::execute_task(&state, &user, &request.task_name, &request.transaction_id)
                .await?,
        ),
    ))
}

async fn dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    let catalog = services::dashboard_catalog(&state).await?;
    let account = &user.account_id;
    let (product_id, item_id) = dashboard_catalog_ids(&catalog)?;
    let wallet = state
        .subscription
        .get(&format!(
            "/v1/accounts/{account}/customer-wallet/statement?limit=50"
        ))
        .await?;
    let eligibility = state
        .subscription
        .get(&format!(
            "/v1/accounts/{account}/products/{}/eligibility",
            product_id
        ))
        .await
        .ok();
    let meter = state
        .subscription
        .get(&format!(
            "/v1/accounts/{account}/items/{}/item-wallet",
            item_id
        ))
        .await
        .ok();
    let item_statement = state
        .subscription
        .get(&format!(
            "/v1/accounts/{account}/items/{}/item-wallet/statement?limit=50",
            item_id
        ))
        .await
        .ok();
    let customer_plan = match user.customer_plan_id.as_deref() {
        Some(plan_id) => state
            .subscription
            .get(&format!(
                "/v1/accounts/{account}/customer-plans/{plan_id}"
            ))
            .await
            .ok(),
        None => None,
    };
    let payment_methods = state.subscription.get(&format!("/v1/accounts/{}/payment-method-bindings", user.account_id)).await.unwrap_or_else(|_| json!([]));
    let checkouts=sqlx::query_as::<_,CheckoutRow>("SELECT checkout_id,checkout_kind,status,amount_minor,currency,granted_credit_units,transaction_id,created_at FROM checkouts WHERE user_id=? ORDER BY created_at DESC,checkout_id DESC LIMIT 20")
        .bind(&user.user_id).fetch_all(&state.pool).await?;
    let executions=sqlx::query_as::<_,ExecutionRow>("SELECT execution_id,task_name,result_text,credits_debited,status,created_at FROM executions WHERE user_id=? ORDER BY created_at DESC LIMIT 20")
        .bind(&user.user_id).fetch_all(&state.pool).await?;
    Ok(Json(
        json!({"account":auth::account_response(user)?,"catalog":catalog,"wallet_statement":wallet,
        "eligibility":eligibility,"meter":meter,"item_statement":item_statement,"customer_plan":customer_plan,"payment_methods":payment_methods,"checkouts":checkouts,"executions":executions}),
    ))
}

fn dashboard_catalog_ids(catalog: &Value) -> Result<(&str, &str), AppError> {
    let product_id = catalog
        .get("product_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_catalog_id("product_id"))?;
    Uuid::parse_str(product_id).map_err(|_| invalid_catalog_id("product_id"))?;
    let item_id = catalog
        .get("item_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_catalog_id("item_id"))?;
    Uuid::parse_str(item_id).map_err(|_| invalid_catalog_id("item_id"))?;
    Ok((product_id, item_id))
}

fn invalid_catalog_id(field: &str) -> AppError {
    AppError::Integration(format!(
        "TaskLab catalog field {field} is invalid: expected a UUID string"
    ))
}

async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let user = auth::current_user(&state, &headers).await?;
    let rows=sqlx::query("SELECT execution_id,task_name,result_text,credits_debited,status,created_at FROM executions WHERE user_id=? ORDER BY created_at DESC LIMIT 50")
        .bind(user.user_id.to_string()).fetch_all(&state.pool).await?;
    let executions:Vec<Value>=rows.into_iter().map(|row|json!({"execution_id":row.get::<String,_>("execution_id"),"task_name":row.get::<String,_>("task_name"),"result_text":row.get::<Option<String>,_>("result_text"),"credits_debited":row.get::<String,_>("credits_debited"),"status":row.get::<String,_>("status"),"created_at":row.get::<String,_>("created_at")})).collect();
    Ok(Json(json!({"executions":executions})))
}

#[derive(FromRow, serde::Serialize)]
struct CheckoutRow {
    checkout_id: String,
    checkout_kind: String,
    status: String,
    amount_minor: Option<i64>,
    currency: Option<String>,
    granted_credit_units: Option<i64>,
    transaction_id: String,
    created_at: String,
}

#[derive(FromRow, serde::Serialize)]
struct ExecutionRow {
    execution_id: String,
    task_name: String,
    result_text: Option<String>,
    credits_debited: String,
    status: String,
    created_at: String,
}

#[cfg(test)]
mod tests {
    use super::dashboard_catalog_ids;
    use serde_json::json;
    use url::Url;

    #[test]
    fn dashboard_catalog_ids_build_unquoted_subscription_paths() {
        let catalog = json!({
            "product_id": "bca1a862-cb9a-4e2a-a20e-9787491d1059",
            "item_id": "e8acb28f-f0b1-4a5f-847b-a1a18e9f2c26"
        });
        let (product_id, item_id) = dashboard_catalog_ids(&catalog).expect("catalog IDs");
        let base = Url::parse("http://localhost:3000/").expect("base URL");
        let eligibility = base
            .join(&format!("v1/products/{product_id}/eligibility"))
            .expect("eligibility URL");
        let item_wallet = base
            .join(&format!("v1/items/{item_id}/item-wallet"))
            .expect("item wallet URL");

        assert!(eligibility
            .as_str()
            .contains("/bca1a862-cb9a-4e2a-a20e-9787491d1059/"));
        assert!(item_wallet
            .as_str()
            .contains("/e8acb28f-f0b1-4a5f-847b-a1a18e9f2c26/"));
        assert!(!eligibility.as_str().contains("%22"));
        assert!(!item_wallet.as_str().contains("%22"));

        let quoted_id = json!({
            "product_id": "\"bca1a862-cb9a-4e2a-a20e-9787491d1059\"",
            "item_id": "e8acb28f-f0b1-4a5f-847b-a1a18e9f2c26"
        });
        assert!(dashboard_catalog_ids(&quoted_id).is_err());
    }
}
