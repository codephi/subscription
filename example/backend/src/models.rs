use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanModel {
    Prepaid,
    Subscription,
}

impl PlanModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepaid => "PREPAID",
            Self::Subscription => "SUBSCRIPTION",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChoosePlanRequest {
    pub plan_model: PlanModel,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateCheckoutRequest {
    pub checkout_kind: CheckoutKind,
    pub topup_credits: Option<i64>,
    pub payment_method_binding_id: Uuid,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SavePaymentMethodBindingRequest {
    pub payment_method_setup_id: Uuid,
    pub card_name: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckoutKind {
    Initial,
    OnDemand,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateExecutionRequest {
    pub task_name: String,
    pub transaction_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AccountResponse {
    pub user_id: Uuid,
    pub username: String,
    pub plan_model: Option<PlanModel>,
    pub customer_plan_id: Option<Uuid>,
    pub workspace_id: Uuid,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckoutResponse {
    pub checkout_id: Uuid,
    pub status: String,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub granted_credit_units: Option<i64>,
    pub transaction_id: String,
}
