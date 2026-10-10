use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::dto::units::CreditUnits;

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateVoucherRequest {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub credit_units: CreditUnits,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub max_total_uses: Option<i64>,
    #[serde(default = "default_account_limit")]
    pub max_uses_per_account: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateCouponRequest {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub discount_kind: String,
    pub discount_value: i64,
    pub currency: Option<String>,
    pub applies_to_initial: bool,
    pub applies_to_on_demand: bool,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub max_total_uses: Option<i64>,
    #[serde(default = "default_account_limit")]
    pub max_uses_per_account: Option<i64>,
}

fn default_account_limit() -> Option<i64> {
    Some(1)
}

#[cfg(test)]
mod tests {
    use super::CreateVoucherRequest;

    #[test]
    fn missing_account_limit_defaults_to_one_but_null_means_unlimited() {
        let base = r#"{"code":"WELCOME","name":"Welcome","credit_units":"5"}"#;
        let defaulted: CreateVoucherRequest = serde_json::from_str(base).unwrap();
        let unlimited: CreateVoucherRequest = serde_json::from_str(
            r#"{"code":"WELCOME","name":"Welcome","credit_units":"5","max_uses_per_account":null}"#,
        )
        .unwrap();
        assert_eq!(defaulted.max_uses_per_account, Some(1));
        assert_eq!(unlimited.max_uses_per_account, None);
    }
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct UpdatePromotionRequest {
    pub status: Option<String>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub max_total_uses: Option<i64>,
    pub max_uses_per_account: Option<i64>,
    pub expected_version: i64,
    pub actor_reference: Option<String>,
}

#[derive(Clone, Debug, Deserialize, IntoParams, ToSchema)]
pub struct PromotionListQuery {
    pub cursor: Option<Uuid>,
    pub status: Option<String>,
    pub search: Option<String>,
    pub limit: Option<u16>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PromotionResponse {
    pub promotion_id: Uuid,
    pub promotion_kind: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: i64,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub max_total_uses: Option<i64>,
    pub max_uses_per_account: Option<i64>,
    pub completed_uses: i64,
    pub reserved_uses: i64,
    pub availability: String,
    pub credit_units: Option<CreditUnits>,
    pub discount_kind: Option<String>,
    pub discount_value: Option<i64>,
    pub currency: Option<String>,
    pub applies_to_initial: Option<bool>,
    pub applies_to_on_demand: Option<bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PromotionPageResponse {
    pub items: Vec<PromotionResponse>,
    pub next_cursor: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PromotionHistoryResponse {
    pub items: Vec<PromotionHistoryEntry>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PromotionHistoryEntry {
    pub version: i64,
    pub action: String,
    pub actor_reference: Option<String>,
    pub before_snapshot: serde_json::Value,
    pub after_snapshot: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct RedeemVoucherRequest {
    pub voucher_id: Option<Uuid>,
    pub code: Option<String>,
    pub transaction_id: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct VoucherRedemptionResponse {
    pub voucher_redemption_id: Uuid,
    pub voucher_id: Uuid,
    pub account_id: Uuid,
    pub credit_units: CreditUnits,
    pub entry: crate::dto::credits::CustomerWalletEntryResponse,
    pub created_at: DateTime<Utc>,
}
