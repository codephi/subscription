use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateBillingConnectionRequest {
    pub provider: String,
    pub external_account_reference: String,
    pub secret_reference: String,
    pub webhook_secret_reference: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct IntegrationProviderResponse {
    pub provider: String,
    pub display_name: String,
    pub available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct WorkspaceIntegrationResponse {
    pub billing_connection_id: Uuid,
    pub provider: String,
    pub account_reference: String,
    pub environment: String,
    pub status: String,
    pub api_secret_configured: bool,
    pub webhook_secret_configured: bool,
    pub customer_reference: Option<String>,
    pub webhook_path: String,
    pub webhook_url: Option<String>,
    pub configuration_version: i32,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateStripeIntegrationRequest {
    pub secret_key: String,
    pub environment: String,
    pub existing_customer_reference: Option<String>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct UpdateStripeIntegrationRequest {
    pub expected_version: i32,
    pub secret_key: Option<String>,
    pub webhook_secret: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct StripeIntegrationTestResponse {
    pub successful: bool,
    pub account_reference: String,
    pub environment: String,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct UpdateDefaultStripeCredentialsRequest {
    pub expected_version: i32,
    pub secret_key: Option<String>,
    pub webhook_secret: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct DefaultStripeCredentialsResponse {
    pub configured: bool,
    pub environment: Option<String>,
    pub account_reference: Option<String>,
    pub api_secret_configured: bool,
    pub webhook_secret_configured: bool,
    pub configuration_version: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct BillingConnectionResponse {
    pub billing_connection_id: Uuid,
    pub workspace_id: Uuid,
    pub provider: String,
    pub external_account_reference: String,
    pub capabilities: Vec<String>,
    pub status: String,
    pub webhook_path: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct BillingCapabilitiesResponse {
    pub payment_methods: Vec<String>,
    pub supports_setup_session: bool,
    pub supports_vault: bool,
    pub supports_off_session_charge: bool,
    pub supports_webhook: bool,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreatePaymentMethodBindingRequest {
    pub customer_plan_id: Uuid,
    pub payment_method_setup_id: Uuid,
    pub card_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreatePaymentMethodFromCardRequest {
    pub customer_plan_id: Uuid,
    pub cardholder_name: String,
    pub card_name: Option<String>,
    pub card_number: String,
    pub exp_month: u8,
    pub exp_year: u16,
    pub cvc: String,
    pub save_for_future: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct CreatePaymentMethodFromCardResponse {
    pub payment_method_binding_id: Option<Uuid>,
    pub saved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct PaymentMethodBindingResponse {
    pub payment_method_binding_id: Uuid,
    pub billing_connection_id: Uuid,
    pub workspace_id: Uuid,
    pub customer_plan_id: Option<Uuid>,
    pub payment_method: String,
    pub display_name: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct CustomerPaymentMethodBindingResponse {
    pub payment_method_binding_id: Uuid,
    pub payment_method: String,
    pub display_name: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

impl From<PaymentMethodBindingResponse> for CustomerPaymentMethodBindingResponse {
    fn from(binding: PaymentMethodBindingResponse) -> Self {
        Self {
            payment_method_binding_id: binding.payment_method_binding_id,
            payment_method: binding.payment_method,
            display_name: binding.display_name,
            status: binding.status,
            created_at: binding.created_at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreatePaymentMethodSetupSessionRequest {
    pub customer_plan_id: Uuid,
    pub success_url: String,
    pub cancel_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct PaymentMethodSetupSessionResponse {
    pub payment_method_setup_id: Uuid,
    pub redirect_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct BillingWebhookResponse {
    pub result: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct BillingOperationsResponse {
    pub pending_collections: i64,
    pub webhook_failures: i64,
    pub unprocessed_webhooks: i64,
    pub open_unmatched_payments: i64,
    pub outbox_backlog: i64,
    pub outbox_dead_letters: i64,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateRenewalRegularizationRequest {
    pub payment_method_binding_id: Uuid,
    pub transaction_id: String,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateInitialCollectionRequest {
    pub payment_method_binding_id: Uuid,
    pub transaction_id: String,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateOnDemandPurchaseRequest {
    pub on_demand_plan_id: Uuid,
    pub payment_method_binding_id: Uuid,
    pub transaction_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct CollectionRequestResponse {
    pub collection_request_id: Uuid,
    pub customer_plan_id: Uuid,
    pub payment_method_binding_id: Uuid,
    pub request_kind: String,
    pub amount_minor: i64,
    pub currency: String,
    pub granted_credit_units: i64,
    pub status: String,
    pub transaction_id: String,
    pub idempotency_key: String,
    pub scheduled_at: DateTime<Utc>,
    pub payment_expires_at: DateTime<Utc>,
    pub coupon_id: Option<Uuid>,
    pub coupon_code: Option<String>,
    pub base_amount_minor: Option<i64>,
    pub discount_amount_minor: i64,
    pub coupon_version: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct UnmatchedPaymentCaseResponse {
    pub unmatched_payment_case_id: Uuid,
    pub workspace_id: Uuid,
    pub billing_connection_id: Uuid,
    pub provider: String,
    pub provider_event_id: String,
    pub provider_payment_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub reason: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}
