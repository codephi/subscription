use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::dto::units::{CreditUnits, ItemUnitBoundary, ItemUnits};

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct DirectCreditRequest {
    pub transaction_id: String,
    pub credit_units: CreditUnits,
    pub external_reference: Option<String>,
    pub description: Option<String>,
    #[schema(value_type = Object)]
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WalletTransactionReferenceResponse {
    pub reference_kind: String,
    pub reference_id: Option<Uuid>,
    pub external_reference: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomerWalletEntryResponse {
    pub customer_wallet_entry_id: Uuid,
    pub customer_wallet_id: Uuid,
    pub customer_id: Uuid,
    pub sequence: i64,
    pub entry_type: String,
    pub source_channel: String,
    pub signed_credit_units: CreditUnits,
    pub balance_before_credit_units: CreditUnits,
    pub balance_after_credit_units: CreditUnits,
    pub transaction_id: Option<String>,
    pub description: Option<String>,
    #[schema(value_type = Object)]
    pub metadata: Value,
    pub request_id: Uuid,
    pub references: Vec<WalletTransactionReferenceResponse>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DirectCreditResponse {
    pub direct_credit_id: Uuid,
    pub credit_lot_id: Uuid,
    pub entry: CustomerWalletEntryResponse,
}

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StatementQuery {
    pub cursor: Option<String>,
    pub limit: Option<u16>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomerWalletStatementResponse {
    pub items: Vec<CustomerWalletEntryResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PendingUsageTransactionResponse {
    pub transaction_id: String,
    pub usage_event_id: Uuid,
    pub item_wallet_entry_id: Uuid,
    pub item_wallet_id: Uuid,
    pub product_id: Uuid,
    pub item_id: Uuid,
    pub received_item_units: ItemUnits,
    pub pending_item_units_after: ItemUnitBoundary,
    #[schema(value_type = Object)]
    pub metadata: Value,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub enum AccountTransactionResponse {
    CustomerWalletEntry(CustomerWalletEntryResponse),
    PendingUsage(PendingUsageTransactionResponse),
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CreditLedgerReconciliationResponse {
    pub account_id: Uuid,
    pub wallet_balance_credit_units: CreditUnits,
    /// Sum of every signed ledger entry, independent of the wallet projection.
    pub ledger_balance_credit_units: CreditUnits,
    /// Remaining credits in lots whose expiration is absent or still in the future.
    pub available_lot_credit_units: CreditUnits,
    pub consistent: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AccountBillingConfigResponse {
    pub account_id: Uuid,
    pub direct_credit_enabled: bool,
    pub recurring_credit_enabled: bool,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct UpdateAccountBillingConfigRequest {
    pub direct_credit_enabled: bool,
    pub recurring_credit_enabled: bool,
    pub expected_version: i64,
}
