use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckoutKind {
    Initial,
    OnDemand,
    PlanUpgrade,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateCheckoutRequest {
    pub customer_plan_id: Uuid,
    pub checkout_kind: CheckoutKind,
    pub on_demand_plan_id: Option<Uuid>,
    #[serde(default)]
    pub target_plan_version_id: Option<Uuid>,
    #[serde(default)]
    pub quantity: Option<i64>,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
    pub transaction_id: String,
    pub coupon_code: Option<String>,
    pub payment_method_binding_id: Option<Uuid>,
    #[serde(default)]
    pub save_payment_method: bool,
    #[serde(default)]
    pub payment_method_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CheckoutQuoteRequest {
    pub customer_plan_id: Uuid,
    pub checkout_kind: CheckoutKind,
    pub on_demand_plan_id: Option<Uuid>,
    #[serde(default)]
    pub target_plan_version_id: Option<Uuid>,
    #[serde(default)]
    pub quantity: Option<i64>,
    pub coupon_code: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct CheckoutQuoteResponse {
    pub customer_plan_id: Uuid,
    pub checkout_kind: CheckoutKind,
    pub base_amount_minor: i64,
    pub discount_amount_minor: i64,
    pub amount_minor: i64,
    pub currency: String,
    pub granted_credit_units: i64,
    pub coupon_id: Uuid,
    pub coupon_code: String,
    pub coupon_version: i64,
    pub payment_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct CheckoutResponse {
    pub checkout_id: Uuid,
    pub customer_plan_id: Uuid,
    pub checkout_kind: CheckoutKind,
    pub status: String,
    pub collection_request_id: Option<Uuid>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub granted_credit_units: Option<i64>,
    pub transaction_id: String,
    pub created_at: DateTime<Utc>,
    pub base_amount_minor: Option<i64>,
    pub discount_amount_minor: i64,
    pub coupon_code: Option<String>,
    pub payment_required: bool,
    pub redirect_url: Option<String>,
}

pub fn checkout_request_hash(request: &CreateCheckoutRequest) -> String {
    let body = serde_json::to_vec(request).expect("checkout request serializes");
    hex_digest(&body)
}

fn hex_digest(value: &[u8]) -> String {
    lower_hex(&Sha256::digest(value))
}

fn lower_hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{checkout_request_hash, CheckoutKind, CreateCheckoutRequest};
    use uuid::Uuid;

    #[test]
    fn checkout_hash_is_stable_and_covers_commercial_terms() {
        let checkout = CreateCheckoutRequest {
            customer_plan_id: Uuid::nil(),
            checkout_kind: CheckoutKind::OnDemand,
            on_demand_plan_id: Some(Uuid::from_u128(1)),
            target_plan_version_id: None,
            quantity: Some(1),
            success_url: None,
            cancel_url: None,
            transaction_id: "operation-1".to_string(),
            coupon_code: None,
            payment_method_binding_id: None,
            save_payment_method: false,
            payment_method_name: None,
        };
        assert_eq!(
            checkout_request_hash(&checkout),
            checkout_request_hash(&checkout)
        );
        let mut changed = checkout.clone();
        changed.on_demand_plan_id = Some(Uuid::from_u128(2));
        assert_ne!(
            checkout_request_hash(&checkout),
            checkout_request_hash(&changed)
        );
    }
}
