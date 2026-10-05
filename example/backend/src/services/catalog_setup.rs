use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use super::{
    activate_item, activate_product, create_plan, create_subscription, save_setting, stored_id_or,
};
use crate::{api::required_uuid, state::AppState};

pub async fn setup_catalog(state: &AppState) -> Result<()> {
    let product_id = stored_id_or(&state.pool, "product_id", create_product(state)).await?;
    save_setting(&state.pool, "product_id", &product_id.to_string()).await?;
    let item_id = stored_id_or(&state.pool, "item_id", create_item(state, product_id)).await?;
    save_setting(&state.pool, "item_id", &item_id.to_string()).await?;
    let price_id = stored_id_or(
        &state.pool,
        "price_version_id",
        create_price(state, item_id),
    )
    .await?;
    save_setting(&state.pool, "price_version_id", &price_id.to_string()).await?;
    publish_price(state, price_id).await?;
    ensure_item_active(state, item_id).await?;
    ensure_product_active(state, product_id).await?;
    let subscription = stored_id_or(
        &state.pool,
        "subscription_id",
        create_subscription(&state.subscription, "Créditos TaskLab"),
    )
    .await?;
    save_setting(&state.pool, "subscription_id", &subscription.to_string()).await?;
    let free_plan = stored_id_or(
        &state.pool,
        "free_plan_id",
        create_plan(
            &state.subscription,
            subscription,
            json!({
                "name":"Teste grátis", "commercial_model":"FREE", "price_amount_minor":null,
                "currency":null,"recurrence":"NONE","admission_policy":"OPEN",
                "accepted_payment_methods":[],"granted_credit_units":"10","product_ids":[product_id]
            }),
        ),
    )
    .await?;
    save_setting(&state.pool, "free_plan_id", &free_plan.to_string()).await?;
    for (price_minor, credits) in [(2000_i64, 100_i64), (4000, 200), (6000, 400)] {
        let key = format!("paid_plan_{credits}_id");
        let plan = stored_id_or(&state.pool, &key, create_plan(&state.subscription, subscription, json!({
            "name":format!("Plano {credits}"), "commercial_model":"PAID", "price_amount_minor":price_minor,
            "currency":"BRL","recurrence":"MONTHLY","admission_policy":"OPEN",
            "accepted_payment_methods":["CARD"],"granted_credit_units":credits.to_string(),"product_ids":[product_id]
        }))).await?;
        save_setting(&state.pool, &key, &plan.to_string()).await?;
    }
    let topup = stored_id_or(
        &state.pool,
        "topup_unit_plan_id",
        create_topup(state, subscription, 1),
    )
    .await?;
    save_setting(&state.pool, "topup_unit_plan_id", &topup.to_string()).await?;
    Ok(())
}

async fn create_product(state: &AppState) -> Result<Uuid> {
    let body=state.subscription.post("/v1/products",None,&json!({"name":"TaskLab","description":"Execução de tarefas por créditos","usage_model":"CREDIT_METERED"})).await?;
    Ok(required_uuid(&body, "product_id")?)
}
async fn create_item(state: &AppState, product: Uuid) -> Result<Uuid> {
    let body=state.subscription.post(&format!("/v1/products/{product}/items"),None,&json!({"name":"Tarefa","parent_item_id":null,"unit_name":"tarefa","quantity_scale":"1"})).await?;
    Ok(required_uuid(&body, "item_id")?)
}
async fn create_price(state: &AppState, item: Uuid) -> Result<Uuid> {
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let body=state.subscription.post(&format!("/v1/items/{item}/price-versions"),None,&json!({"pricing_model":"unit","unit_block_size":"1","credit_units":"1","effective_from":now,"effective_until":null,"accumulation_cycle":null,"tiers":[]})).await?;
    Ok(required_uuid(&body, "price_version_id")?)
}
async fn publish_price(state: &AppState, id: Uuid) -> Result<()> {
    let body = state
        .subscription
        .get(&format!("/v1/price-versions/{id}"))
        .await?;
    if body["state"] != "ACTIVE" {
        state
            .subscription
            .post::<Value>(
                &format!("/v1/price-versions/{id}/publish"),
                None,
                &json!({}),
            )
            .await?;
    }
    Ok(())
}
async fn ensure_item_active(state: &AppState, id: Uuid) -> Result<()> {
    let body = state.subscription.get(&format!("/v1/items/{id}")).await?;
    if body["status"] != "ACTIVE" {
        activate_item(
            &state.subscription,
            id,
            body["version"].as_i64().unwrap_or(1),
        )
        .await?;
    }
    Ok(())
}
async fn ensure_product_active(state: &AppState, id: Uuid) -> Result<()> {
    let body = state
        .subscription
        .get(&format!("/v1/products/{id}"))
        .await?;
    if body["status"] != "ACTIVE" {
        activate_product(
            &state.subscription,
            id,
            body["version"].as_i64().unwrap_or(1),
        )
        .await?;
    }
    Ok(())
}
async fn create_topup(state: &AppState, prepaid: Uuid, credits: i64) -> Result<Uuid> {
    let body=state.subscription.post(&format!("/v1/subscriptions/{prepaid}/on-demand-plans"),None,&json!({
        "name":"Crédito avulso", "price_amount_minor":100,"currency":"BRL","credit_units":credits.to_string()
    })).await?;
    Ok(required_uuid(&body, "on_demand_plan_id")?)
}
