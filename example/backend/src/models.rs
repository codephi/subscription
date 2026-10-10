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
    pub payment_method_binding_id: Option<Uuid>,
    pub target_plan_version_id: Option<Uuid>,
    #[serde(default)]
    pub save_payment_method: bool,
    pub payment_method_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaymentMethodSetupRequest {
    pub card_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfirmPaymentMethodRequest {
    pub payment_method_setup_id: Uuid,
    pub card_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RenamePaymentMethodRequest {
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateRegularizationRequest {
    pub transaction_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckoutKind {
    Initial,
    OnDemand,
    PlanUpgrade,
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
    pub account_id: Uuid,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckoutResponse {
    pub checkout_id: Uuid,
    pub status: String,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub granted_credit_units: Option<i64>,
    pub transaction_id: String,
    pub redirect_url: Option<String>,
}
