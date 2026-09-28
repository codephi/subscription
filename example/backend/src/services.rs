use anyhow::Result;
use base64::Engine as _;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    api::{required_uuid, SubscriptionClient},
    auth::AuthenticatedUser,
    errors::AppError,
    models::{CheckoutKind, CheckoutResponse, PlanModel},
    state::AppState,
};

mod catalog_setup;
pub use catalog_setup::setup_catalog;

pub async fn provision_workspace(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<(), AppError> {
    let workspace =
        Uuid::parse_str(&user.workspace_id).map_err(|error| AppError::Internal(error.into()))?;
    let correlation =
        Uuid::parse_str(&user.correlation_id).map_err(|error| AppError::Internal(error.into()))?;
    let occurred = chrono::DateTime::parse_from_rfc3339(&user.event_occurred_at)
        .map_err(|error| AppError::Internal(error.into()))?
        .with_timezone(&Utc);
    let events = [
        (
            user.created_event_id.as_str(),
            "workspace.created",
            1_i64,
            None,
        ),
        (
            user.activated_event_id.as_str(),
            "workspace.activated",
            2_i64,
            Some(user.created_event_id.as_str()),
        ),
    ];
    for (event_id, event_type, sequence, causation) in events {
        let envelope = json!({"event_id":event_id,"event_type":event_type,"schema_version":1,
            "aggregate_id":workspace,"sequence":sequence,"occurred_at":occurred,
            "workspace_id":workspace,"correlation_id":correlation,"causation_id":causation,
            "payload":{"workspace_id":workspace}});
        let body =
            serde_json::to_vec(&envelope).map_err(|error| AppError::Internal(error.into()))?;
        let timestamp = Utc::now().timestamp().to_string();
        let signature = sign_body(&state.accounts_webhook_secret, &timestamp, &body)?;
        state
            .subscription
            .post_signed(
                "/v1/internal/accounts/workspace-events",
                &body,
                &timestamp,
                &signature,
            )
            .await?;
    }
    Ok(())
}

pub async fn choose_plan(
    state: &AppState,
    user: &AuthenticatedUser,
    model: PlanModel,
) -> Result<Value, AppError> {
    let (plan_key, transaction) = match model {
        PlanModel::Prepaid => (
            "free_plan_id",
            format!("tasklab-free:{}", user.workspace_id),
        ),
        PlanModel::Subscription => {
            return Err(AppError::Invalid(
                "assinatura é ativada no checkout inicial".into(),
            ))
        }
    };
    let plan_id = setting(&state.pool, plan_key)
        .await?
        .ok_or_else(catalog_missing)?;
    let customer_plan = create_customer_plan(state, user, &plan_id, &transaction).await?;
    sqlx::query(
        "UPDATE users SET plan_model=?,customer_plan_id=? WHERE user_id=? AND plan_model IS NULL",
    )
    .bind(model.as_str())
    .bind(customer_plan.to_string())
    .bind(&user.user_id)
    .execute(&state.pool)
    .await?;
    Ok(json!({"plan_model":model,"customer_plan_id":customer_plan}))
}

pub async fn create_checkout(
    state: &AppState,
    user: &AuthenticatedUser,
    kind: CheckoutKind,
    topup_credits: Option<i64>,
    payment_method_binding_id: Uuid,
    transaction: String,
) -> Result<CheckoutResponse, AppError> {
    if transaction.trim().is_empty() || transaction.len() > 128 {
        return Err(AppError::Invalid(
            "transaction_id deve ter entre 1 e 128 caracteres".into(),
        ));
    }
    let workspace = &user.workspace_id;
    let (plan_id, model) = match kind {
        CheckoutKind::OnDemand => (
            user.customer_plan_id
                .clone()
                .ok_or_else(|| AppError::Conflict("escolha pré-pago antes de recarregar".into()))?,
            user.plan_model.as_deref(),
        ),
        CheckoutKind::Initial => {
            if user
                .plan_model
                .as_deref()
                .is_some_and(|selected| selected != "SUBSCRIPTION")
            {
                return Err(AppError::Conflict(
                    "conta pré-paga não pode trocar de modalidade nesta POC".into(),
                ));
            }
            let customer_plan = match user.customer_plan_id.as_deref() {
                Some(id) => Uuid::parse_str(id)
                    .map_err(|error| AppError::Integration(error.to_string()))?,
                None => {
                    let id = setting(&state.pool, "paid_plan_id")
                        .await?
                        .ok_or_else(catalog_missing)?;
                    let customer_plan = create_customer_plan(state, user, &id, &transaction).await?;
                    sqlx::query(
                        "UPDATE users SET plan_model='SUBSCRIPTION',customer_plan_id=? WHERE user_id=? AND customer_plan_id IS NULL",
                    )
                    .bind(customer_plan.to_string())
                    .bind(&user.user_id)
                    .execute(&state.pool)
                    .await?;
                    customer_plan
                }
            };
            (customer_plan.to_string(), Some("SUBSCRIPTION"))
        }
    };
    let key = format!("tasklab-checkout:{}:{transaction}", user.user_id);
    let request = match kind {
        CheckoutKind::Initial => {
            json!({"checkout_kind":"INITIAL","customer_plan_id":plan_id,"transaction_id":transaction,"payment_method_binding_id":payment_method_binding_id})
        }
        CheckoutKind::OnDemand => {
            let credits = topup_credits.unwrap_or(10);
            let topup_plan = topup_plan_id(&state.pool, credits).await?;
            json!({"checkout_kind":"ON_DEMAND","customer_plan_id":plan_id,"on_demand_plan_id":topup_plan,"transaction_id":transaction,"payment_method_binding_id":payment_method_binding_id})
        }
    };
    let body = state
        .subscription
        .post(
            &format!("/v1/workspaces/{workspace}/checkouts"),
            Some(&key),
            &request,
        )
        .await?;
    let checkout_id = required_uuid(&body, "checkout_id").map_err(integration_error)?;
    let response = CheckoutResponse {
        checkout_id,
        status: body["status"].as_str().unwrap_or("PENDING").to_string(),
        amount_minor: body["amount_minor"].as_i64(),
        currency: body["currency"].as_str().map(str::to_owned),
        granted_credit_units: body["granted_credit_units"].as_i64(),
        transaction_id: transaction,
    };
    sqlx::query("INSERT INTO checkouts(checkout_id,user_id,transaction_id,subscription_checkout_id,checkout_kind,status,amount_minor,currency,granted_credit_units) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(subscription_checkout_id) DO UPDATE SET status=excluded.status")
        .bind(response.checkout_id.to_string()).bind(&user.user_id).bind(&response.transaction_id).bind(response.checkout_id.to_string())
        .bind(format!("{kind:?}" )).bind(&response.status).bind(response.amount_minor).bind(&response.currency).bind(response.granted_credit_units)
        .execute(&state.pool).await?;
    let _ = model;
    Ok(response)
}

pub async fn create_payment_method_setup(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<Value, AppError> {
    let publishable_key = state
        .stripe_publishable_key
        .as_deref()
        .filter(|key| key.starts_with("pk_test_") || key.starts_with("pk_live_"))
        .ok_or_else(|| AppError::Invalid("configure STRIPE_PUBLISHABLE_KEY para tokenizar cartões".into()))?;
    let customer_plan_id = ensure_payment_customer_plan(state, user).await?;
    let integrations = state
        .subscription
        .get(&format!("/v1/admin/workspaces/{}/integrations", user.workspace_id))
        .await?;
    let connection_id = integrations
        .as_array()
        .and_then(|items| items.iter().find(|item| {
            item["provider"] == "STRIPE" && item["status"] == "ACTIVE"
        }))
        .and_then(|item| item["billing_connection_id"].as_str())
        .ok_or_else(|| AppError::Integration("não existe uma integração Stripe ativa para esta conta".into()))?;
    let session = state
        .subscription
        .post(
            &format!("/v1/workspaces/{}/billing-connections/{connection_id}/payment-method-setup-sessions", user.workspace_id),
            None,
            &json!({"customer_plan_id":customer_plan_id}),
        )
        .await?;
    Ok(json!({
        "billing_connection_id":connection_id,
        "customer_plan_id":customer_plan_id,
        "setup_intent_id":session["provider_setup_id"],
        "client_secret":session["client_secret"],
        "publishable_key":publishable_key,
    }))
}

pub async fn save_payment_method_binding(
    state: &AppState,
    user: &AuthenticatedUser,
    setup_intent_id: String,
) -> Result<Value, AppError> {
    if setup_intent_id.len() > 255 || !setup_intent_id.starts_with("seti_") {
        return Err(AppError::Invalid("setup_intent_id deve ser uma referência seti_ válida".into()));
    }
    let customer_plan_id = ensure_payment_customer_plan(state, user).await?;
    let integration = state
        .subscription
        .get(&format!("/v1/admin/workspaces/{}/integrations", user.workspace_id))
        .await?;
    let connection_id = integration
        .as_array()
        .and_then(|items| items.iter().find(|item| item["provider"] == "STRIPE" && item["status"] == "ACTIVE"))
        .and_then(|item| item["billing_connection_id"].as_str())
        .ok_or_else(|| AppError::Integration("não existe uma integração Stripe ativa para esta conta".into()))?;
    state
        .subscription
        .post(
            &format!("/v1/workspaces/{}/payment-method-bindings", user.workspace_id),
            None,
            &json!({"billing_connection_id":connection_id,"customer_plan_id":customer_plan_id,"setup_intent_id":setup_intent_id}),
        )
        .await
}

pub async fn list_payment_method_bindings(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<Value, AppError> {
    state
        .subscription
        .get(&format!("/v1/workspaces/{}/payment-method-bindings", user.workspace_id))
        .await
}

async fn ensure_payment_customer_plan(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<Uuid, AppError> {
    if let Some(customer_plan_id) = user.customer_plan_id.as_deref() {
        return Uuid::parse_str(customer_plan_id)
            .map_err(|error| AppError::Integration(error.to_string()));
    }
    let plan_id = setting(&state.pool, "paid_plan_id").await?.ok_or_else(catalog_missing)?;
    let transaction = format!("tasklab-card-setup:{}", user.user_id);
    let customer_plan_id = create_customer_plan(state, user, &plan_id, &transaction).await?;
    sqlx::query("UPDATE users SET plan_model='SUBSCRIPTION',customer_plan_id=? WHERE user_id=? AND customer_plan_id IS NULL")
        .bind(customer_plan_id.to_string())
        .bind(&user.user_id)
        .execute(&state.pool)
        .await?;
    Ok(customer_plan_id)
}

pub async fn refresh_checkout(
    state: &AppState,
    user: &AuthenticatedUser,
    checkout_id: Uuid,
) -> Result<CheckoutResponse, AppError> {
    let row = sqlx::query_as::<_, (String,String)>("SELECT subscription_checkout_id,transaction_id FROM checkouts WHERE checkout_id=? AND user_id=?")
        .bind(checkout_id.to_string()).bind(&user.user_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::Invalid("checkout não encontrado".into()))?;
    let body = state
        .subscription
        .get(&format!(
            "/v1/workspaces/{}/checkouts/{}",
            user.workspace_id, row.0
        ))
        .await?;
    let response = CheckoutResponse {
        checkout_id,
        status: body["status"].as_str().unwrap_or("PENDING").to_string(),
        amount_minor: body["amount_minor"].as_i64(),
        currency: body["currency"].as_str().map(str::to_owned),
        granted_credit_units: body["granted_credit_units"].as_i64(),
        transaction_id: row.1,
    };
    sqlx::query("UPDATE checkouts SET status=?,amount_minor=?,currency=?,granted_credit_units=? WHERE checkout_id=?")
        .bind(&response.status).bind(response.amount_minor).bind(&response.currency).bind(response.granted_credit_units).bind(checkout_id.to_string()).execute(&state.pool).await?;
    Ok(response)
}

pub async fn execute_task(
    state: &AppState,
    user: &AuthenticatedUser,
    name: &str,
    transaction: &str,
) -> Result<Value, AppError> {
    let task = name.trim();
    if task.is_empty() || task.len() > 120 {
        return Err(AppError::Invalid(
            "nome da tarefa deve ter entre 1 e 120 caracteres".into(),
        ));
    }
    if transaction.trim().is_empty() || transaction.len() > 128 || !transaction.is_ascii() {
        return Err(AppError::Invalid(
            "transaction_id deve conter de 1 a 128 caracteres ASCII".into(),
        ));
    }
    if let Some(existing) = sqlx::query_as::<_, (String,String,String)>("SELECT execution_id,task_name,result_text FROM executions WHERE user_id=? AND transaction_id=? AND status='COMPLETED'")
        .bind(&user.user_id).bind(transaction).fetch_optional(&state.pool).await? {
        return Ok(json!({"execution_id":existing.0,"task_name":existing.1,"result_text":existing.2,"debited_credit_units":"1"}));
    }
    let catalog = catalog_values(&state.pool).await?;
    let eligibility = state
        .subscription
        .get(&format!(
            "/v1/workspaces/{}/products/{}/eligibility",
            user.workspace_id, catalog.product
        ))
        .await?;
    let meter = state
        .subscription
        .get(&format!(
            "/v1/workspaces/{}/items/{}/item-wallet",
            user.workspace_id, catalog.item
        ))
        .await?;
    if eligibility["access_allowed"] != true
        || meter["next_block_credit_units"]
            .as_str()
            .unwrap_or("0")
            .parse::<i64>()
            .unwrap_or(i64::MAX)
            > 1
    {
        return Err(AppError::Conflict(
            "saldo ou elegibilidade insuficiente".into(),
        ));
    }
    let execution_id = Uuid::new_v4();
    sqlx::query("INSERT INTO executions(execution_id,user_id,transaction_id,task_name,result_text,credits_debited,status) VALUES(?,?,?,?,NULL,'1','PENDING') ON CONFLICT(user_id,transaction_id) DO NOTHING")
        .bind(execution_id.to_string()).bind(&user.user_id).bind(transaction).bind(task).execute(&state.pool).await?;
    let usage_key = format!("tasklab-usage:{}:{transaction}", user.user_id);
    let response = match state.subscription.post(&format!("/v1/workspaces/{}/usage-events",user.workspace_id),Some(&usage_key),&json!({
        "transaction_id":transaction,"product_id":catalog.product,"item_id":catalog.item,"item_units":"1",
        "expected_price_version_id":meter["next_price_version_id"],"occurred_at":null,"metadata":{"tasklab_execution_id":execution_id}
    })).await {
        Ok(response) => response,
        Err(error) => {
            if matches!(error, AppError::Conflict(_) | AppError::Invalid(_)) {
                sqlx::query("UPDATE executions SET status='REJECTED',result_text=? WHERE user_id=? AND transaction_id=?")
                    .bind(error.to_string()).bind(&user.user_id).bind(transaction).execute(&state.pool).await?;
            }
            return Err(error);
        }
    };
    let result = format!(
        "Tarefa '{}' concluída com sucesso. Identificador {}.",
        task,
        &transaction.chars().take(8).collect::<String>()
    );
    sqlx::query("UPDATE executions SET status='COMPLETED',result_text=? WHERE user_id=? AND transaction_id=?")
        .bind(&result).bind(&user.user_id).bind(transaction).execute(&state.pool).await?;
    Ok(
        json!({"execution_id":execution_id,"task_name":task,"result_text":result,"debited_credit_units":response["debited_credit_units"],"balance_after_credit_units":response["balance_after_credit_units"]}),
    )
}

async fn create_customer_plan(
    state: &AppState,
    user: &AuthenticatedUser,
    plan_id: &str,
    transaction: &str,
) -> Result<Uuid, AppError> {
    let billing = state
        .subscription
        .get(&format!(
            "/v1/workspaces/{}/billing-config",
            user.workspace_id
        ))
        .await?;
    if billing["recurring_credit_enabled"] != true {
        state.subscription.put(&format!("/v1/workspaces/{}/billing-config",user.workspace_id),&json!({
            "direct_credit_enabled":billing["direct_credit_enabled"],"recurring_credit_enabled":true,"expected_version":billing["version"]
        })).await?;
    }
    let body = state
        .subscription
        .post(
            &format!("/v1/workspaces/{}/customer-plans", user.workspace_id),
            Some(&format!("tasklab-plan:{}", transaction)),
            &json!({"plan_version_id":plan_id,"transaction_id":transaction}),
        )
        .await?;
    required_uuid(&body, "customer_plan_id").map_err(integration_error)
}

async fn create_subscription(api: &SubscriptionClient, name: &str) -> Result<Uuid> {
    let b = api
        .post(
            "/v1/subscriptions",
            None,
            &json!({"name":name,"subscription_model":"CREDIT_STRICT"}),
        )
        .await?;
    Ok(required_uuid(&b, "subscription_id")?)
}
async fn create_plan(api: &SubscriptionClient, subscription: Uuid, body: Value) -> Result<Uuid> {
    let b = api
        .post(
            &format!("/v1/subscriptions/{subscription}/plans"),
            None,
            &body,
        )
        .await?;
    Ok(required_uuid(&b, "plan_version_id")?)
}
async fn activate_item(api: &SubscriptionClient, id: Uuid, version: i64) -> Result<()> {
    api.patch(
        &format!("/v1/items/{id}"),
        &json!({"status":"ACTIVE","expected_version":version}),
    )
    .await?;
    Ok(())
}
async fn activate_product(api: &SubscriptionClient, id: Uuid, version: i64) -> Result<()> {
    api.patch(
        &format!("/v1/products/{id}"),
        &json!({"status":"ACTIVE","expected_version":version}),
    )
    .await?;
    Ok(())
}
async fn save_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO catalog_settings(setting_key,setting_value) VALUES(?,?) ON CONFLICT(setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=CURRENT_TIMESTAMP").bind(key).bind(value).execute(pool).await?;
    Ok(())
}
async fn setting(pool: &SqlitePool, key: &str) -> Result<Option<String>, AppError> {
    Ok(
        sqlx::query_scalar("SELECT setting_value FROM catalog_settings WHERE setting_key=?")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}
async fn setting_uuid(pool: &SqlitePool, key: &str) -> Result<Option<Uuid>, AppError> {
    setting(pool, key)
        .await?
        .map(|value| Uuid::parse_str(&value).map_err(|error| AppError::Internal(error.into())))
        .transpose()
}
async fn stored_id_or<F>(pool: &SqlitePool, key: &str, create: F) -> Result<Uuid>
where
    F: std::future::Future<Output = Result<Uuid>>,
{
    if let Some(id) = setting_uuid(pool, key).await? {
        return Ok(id);
    }
    let id = create.await?;
    save_setting(pool, key, &id.to_string()).await?;
    Ok(id)
}
fn sign_body(secret: &str, timestamp: &str, body: &[u8]) -> Result<String, AppError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|error| AppError::Internal(error.into()))?;
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    Ok(format!(
        "v1={}",
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    ))
}
fn catalog_missing() -> AppError {
    AppError::Integration("execute npm run setup antes de iniciar a POC".into())
}
fn integration_error(error: AppError) -> AppError {
    AppError::Integration(error.to_string())
}
struct CatalogValues {
    product: Uuid,
    item: Uuid,
}
async fn catalog_values(pool: &SqlitePool) -> Result<CatalogValues, AppError> {
    Ok(CatalogValues {
        product: Uuid::parse_str(
            &setting(pool, "product_id")
                .await?
                .ok_or_else(catalog_missing)?,
        )
        .map_err(|error| AppError::Internal(error.into()))?,
        item: Uuid::parse_str(
            &setting(pool, "item_id")
                .await?
                .ok_or_else(catalog_missing)?,
        )
        .map_err(|error| AppError::Internal(error.into()))?,
    })
}

pub async fn dashboard_catalog(state: &AppState) -> Result<Value, AppError> {
    let product = setting(&state.pool, "product_id")
        .await?
        .ok_or_else(catalog_missing)?;
    let item = setting(&state.pool, "item_id")
        .await?
        .ok_or_else(catalog_missing)?;
    Ok(
        json!({"product_id":product,"item_id":item,"prepaid_price_minor":1000,"prepaid_credits":10,"subscription_price_minor":2990,"subscription_credits":50,"task_cost":1,"topup_offers":[{"credit_units":10,"price_amount_minor":1000},{"credit_units":25,"price_amount_minor":2500},{"credit_units":50,"price_amount_minor":5000}]}),
    )
}

async fn topup_plan_id(pool: &SqlitePool, credits: i64) -> Result<String, AppError> {
    let key = topup_plan_setting_key(credits)?;
    setting(pool, key).await?.ok_or_else(catalog_missing)
}

fn topup_plan_setting_key(credits: i64) -> Result<&'static str, AppError> {
    match credits {
        10 => Ok("topup_10_plan_id"),
        25 => Ok("topup_25_plan_id"),
        50 => Ok("topup_50_plan_id"),
        _ => Err(AppError::Invalid(
            "pacote de recarga inválido; escolha 10, 25 ou 50 créditos".into(),
        )),
    }
}

#[cfg(test)]
mod topup_tests {
    use super::topup_plan_setting_key;

    #[test]
    fn only_persisted_topup_packages_are_selectable() {
        assert_eq!(topup_plan_setting_key(10).unwrap(), "topup_10_plan_id");
        assert_eq!(topup_plan_setting_key(25).unwrap(), "topup_25_plan_id");
        assert_eq!(topup_plan_setting_key(50).unwrap(), "topup_50_plan_id");
        assert!(topup_plan_setting_key(11).is_err());
    }
}
