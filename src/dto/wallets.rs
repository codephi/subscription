use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::units::CreditUnits;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WalletStatus {
    Provisioning,
    Active,
    Disabled,
    Error,
}

impl WalletStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provisioning => "PROVISIONING",
            Self::Active => "ACTIVE",
            Self::Disabled => "DISABLED",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomerWalletResponse {
    pub wallet_id: Uuid,
    pub customer_id: Uuid,
    pub balance_credit_units: CreditUnits,
    pub status: WalletStatus,
    pub version: i64,
    pub provisioning_scope_version: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ItemWalletResponse {
    pub wallet_id: Uuid,
    pub customer_id: Uuid,
    pub parent_customer_wallet_id: Uuid,
    pub item_id: Uuid,
    pub status: WalletStatus,
    pub provisioning_scope_version: Uuid,
    pub total_received_item_units: String,
    pub total_converted_item_units: String,
    pub pending_item_units: String,
    pub version: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WalletHierarchyResponse {
    pub workspace_id: Uuid,
    pub scope_version: Uuid,
    pub ready: bool,
    pub customer_wallet: CustomerWalletResponse,
    pub item_wallets: Vec<ItemWalletResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WalletProvisioningResponse {
    pub workspace_id: Uuid,
    pub scope_version: Uuid,
    pub status: WalletStatus,
    pub expected_item_wallets: i64,
    pub materialized_item_wallets: i64,
    pub error_detail: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}
