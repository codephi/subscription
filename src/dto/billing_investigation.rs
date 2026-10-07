use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillingRecordQuery {
    pub cursor: Option<Uuid>,
    pub limit: Option<u16>,
    pub account_id: Option<Uuid>,
    pub collection_request_id: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BillingRecordResponse {
    pub id: Uuid,
    pub kind: String,
    pub account_id: Option<Uuid>,
    pub status: String,
    pub occurred_at: DateTime<Utc>,
    pub collection_request_id: Option<Uuid>,
    pub collection_attempt_id: Option<Uuid>,
    pub billing_payment_id: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    pub provider_event_id: Option<String>,
    pub provider_payment_id: Option<String>,
    pub event_type: Option<String>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub failure_code: Option<String>,
    pub detail: Option<String>,
    pub coupon_id: Option<Uuid>,
    pub coupon_code: Option<String>,
    pub base_amount_minor: Option<i64>,
    pub discount_amount_minor: Option<i64>,
    pub coupon_version: Option<i64>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BillingRecordPageResponse {
    pub items: Vec<BillingRecordResponse>,
    pub next_cursor: Option<Uuid>,
}
