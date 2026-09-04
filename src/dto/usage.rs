use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::dto::units::{CreditUnits, ItemUnitBoundary, ItemUnits};

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateUsageEventRequest {
    pub transaction_id: String,
    pub product_id: Uuid,
    pub item_id: Uuid,
    pub item_units: ItemUnits,
    pub expected_price_version_id: Option<Uuid>,
    pub occurred_at: Option<DateTime<Utc>>,
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct UsageAllocationResponse {
    pub price_version_id: Uuid,
    pub pricing_model: String,
    pub tier_position: Option<i32>,
    pub unit_block_size: ItemUnits,
    pub cycle_key: String,
    pub accumulated_units_before: ItemUnitBoundary,
    pub accumulated_units_after: ItemUnitBoundary,
    pub converted_blocks: i64,
    pub debited_credit_units: CreditUnits,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct UsageEventResponse {
    pub usage_event_id: Uuid,
    pub item_wallet_id: Uuid,
    pub item_wallet_entry_id: Uuid,
    pub debit_id: Option<Uuid>,
    pub customer_wallet_entry_id: Option<Uuid>,
    pub transaction_id: String,
    pub received_item_units: ItemUnits,
    pub pending_item_units_before: ItemUnitBoundary,
    pub converted_item_units: ItemUnitBoundary,
    pub converted_blocks: i64,
    pub pending_item_units_after: ItemUnitBoundary,
    pub allocations: Vec<UsageAllocationResponse>,
    pub debited_credit_units: CreditUnits,
    pub balance_before_credit_units: Option<CreditUnits>,
    pub balance_after_credit_units: Option<CreditUnits>,
    pub billing_status: String,
    pub product_eligible_after: bool,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProductEligibilityResponse {
    pub product_id: Uuid,
    pub usage_model: String,
    pub eligible: bool,
    pub reason: String,
    pub commercial_status: Option<String>,
    pub renewal_status: Option<String>,
    pub entitled: bool,
    pub balance_credit_units: Option<CreditUnits>,
    pub wallet_version: Option<i64>,
    pub evaluated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ItemWalletMeterResponse {
    pub customer_id: Uuid,
    pub product_id: Uuid,
    pub item_id: Uuid,
    pub item_wallet_id: Uuid,
    pub parent_customer_wallet_id: Uuid,
    pub total_received_item_units: ItemUnitBoundary,
    pub total_converted_item_units: ItemUnitBoundary,
    pub total_converted_blocks: i64,
    pub pending_item_units: ItemUnitBoundary,
    pub pending_price_version_id: Option<Uuid>,
    pub pending_tier_position: Option<i32>,
    pub pending_unit_block_size: Option<ItemUnits>,
    pub next_price_version_id: Uuid,
    pub units_until_next_block: ItemUnits,
    pub next_block_credit_units: CreditUnits,
    pub pricing_accumulators: Vec<PricingAccumulatorResponse>,
    pub version: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ItemStatementQuery {
    pub cursor: Option<String>,
    pub limit: Option<u16>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ItemWalletEntryResponse {
    pub item_wallet_entry_id: Uuid,
    pub usage_event_id: Uuid,
    pub product_id: Uuid,
    pub item_id: Uuid,
    pub transaction_id: String,
    pub received_item_units: ItemUnits,
    pub total_received_item_units_before: ItemUnitBoundary,
    pub total_received_item_units_after: ItemUnitBoundary,
    pub converted_item_units: ItemUnitBoundary,
    pub converted_blocks: i64,
    pub pending_item_units_after: ItemUnitBoundary,
    pub emitted_debited_credit_units: CreditUnits,
    pub debit_id: Option<Uuid>,
    pub customer_wallet_entry_id: Option<Uuid>,
    pub billing_block_ids: Vec<Uuid>,
    pub billing_blocks: Vec<BillingBlockResponse>,
    #[schema(value_type = Object)]
    pub metadata: Value,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BillingBlockResponse {
    pub billing_block_id: Uuid,
    pub global_block_sequence: i64,
    pub price_version_id: Uuid,
    pub price_block_ordinal: i64,
    pub tier_position: Option<i32>,
    pub cycle_key: String,
    pub cycle_start: Option<DateTime<Utc>>,
    pub cycle_end: Option<DateTime<Utc>>,
    pub accumulated_units_before: ItemUnitBoundary,
    pub accumulated_units_after: ItemUnitBoundary,
    pub unit_block_size: ItemUnits,
    pub debited_credit_units: CreditUnits,
    pub unit_offset_start: ItemUnitBoundary,
    pub unit_offset_end: ItemUnitBoundary,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ItemWalletStatementResponse {
    pub items: Vec<ItemWalletEntryResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PricingAccumulatorResponse {
    pub pricing_accumulator_id: Uuid,
    pub price_version_id: Uuid,
    pub cycle_key: String,
    pub accumulated_converted_item_units: ItemUnitBoundary,
    pub converted_blocks: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PricingAccumulatorQuery {
    pub price_version_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct UsageReconciliationResponse {
    pub customer_id: Uuid,
    pub item_id: Uuid,
    pub meter_received_item_units: ItemUnitBoundary,
    pub statement_received_item_units: ItemUnitBoundary,
    pub meter_converted_item_units: ItemUnitBoundary,
    pub block_converted_item_units: ItemUnitBoundary,
    pub meter_pending_item_units: ItemUnitBoundary,
    pub allocated_credit_units: CreditUnits,
    pub debited_credit_units: CreditUnits,
    pub consistent: bool,
}
