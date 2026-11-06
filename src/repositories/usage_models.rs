use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub(super) struct LockedMeter {
    pub wallet_id: Uuid,
    pub customer_wallet_id: Uuid,
    pub total_received: i64,
    pub total_converted: i64,
    pub total_blocks: i64,
    pub pending: i64,
    pub pending_price_id: Option<Uuid>,
    pub pending_tier_position: Option<i32>,
    pub pending_block_size: Option<i64>,
    pub pending_credit_units: Option<i64>,
    pub version: i64,
}

#[derive(Clone)]
pub(super) struct PriceDefinition {
    pub price_id: Uuid,
    pub model: String,
    pub block_size: Option<i64>,
    pub credit_units: Option<i64>,
    pub anchor_at: Option<DateTime<Utc>>,
    pub recurrence_rule: Option<String>,
    pub tiers: Vec<PriceTier>,
}

#[derive(Clone)]
pub(super) struct PriceTier {
    pub position: i32,
    pub from: i64,
    pub to: Option<i64>,
    pub block_size: i64,
    pub credit_units: i64,
}

#[derive(Clone)]
pub(super) struct CycleWindow {
    pub key: String,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub anchor_at: Option<DateTime<Utc>>,
    pub recurrence_rule: Option<String>,
}

#[derive(Clone)]
pub(super) struct PricedBlock {
    pub price_id: Uuid,
    pub model: String,
    pub tier_position: Option<i32>,
    pub block_size: i64,
    pub credit_units: i64,
    pub cycle: CycleWindow,
    pub accumulated_before: i64,
    pub accumulated_after: i64,
    pub price_block_ordinal: i64,
}

#[derive(Clone)]
pub(super) struct PendingQuote {
    pub price_id: Uuid,
    pub tier_position: Option<i32>,
    pub block_size: i64,
    pub credit_units: i64,
}

pub(super) struct Conversion {
    pub blocks: Vec<PricedBlock>,
    pub converted_units: i64,
    pub pending_after: i64,
    pub pending_quote: Option<PendingQuote>,
    pub debited_credits: i64,
}

pub(super) struct DebitResult {
    pub debit_id: Uuid,
    pub entry_id: Uuid,
    pub entry_sequence: i64,
    pub balance_before: i64,
    pub balance_after: i64,
}
