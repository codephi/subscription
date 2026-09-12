use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateRenewalRegularizationRequest {
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
}
