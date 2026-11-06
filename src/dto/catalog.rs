use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::units::{CreditUnits, ItemUnitBoundary, ItemUnits};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UsageModel {
    CreditMetered,
    EntitlementOnly,
}

impl UsageModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreditMetered => "CREDIT_METERED",
            Self::EntitlementOnly => "ENTITLEMENT_ONLY",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CatalogStatus {
    Active,
    Inactive,
    Archived,
}

impl CatalogStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
            Self::Archived => "ARCHIVED",
        }
    }
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateProductRequest {
    pub name: String,
    pub description: Option<String>,
    pub usage_model: UsageModel,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct UpdateProductRequest {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub description: Option<Option<String>>,
    pub usage_model: Option<UsageModel>,
    pub status: Option<CatalogStatus>,
    pub expected_version: i64,
}

fn deserialize_optional_field<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProductResponse {
    pub product_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub usage_model: UsageModel,
    pub status: CatalogStatus,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateItemRequest {
    pub name: String,
    pub parent_item_id: Option<Uuid>,
    pub unit_name: Option<String>,
    pub quantity_scale: Option<ItemUnits>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct UpdateItemRequest {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub parent_item_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub unit_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub quantity_scale: Option<Option<ItemUnits>>,
    pub status: Option<CatalogStatus>,
    pub expected_version: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ItemResponse {
    pub item_id: Uuid,
    pub product_id: Uuid,
    pub parent_item_id: Option<Uuid>,
    pub name: String,
    pub unit_name: Option<String>,
    pub quantity_scale: Option<ItemUnits>,
    pub status: CatalogStatus,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PricingModel {
    Unit,
    Tiered,
}

impl PricingModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Tiered => "tiered",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AccumulationCycleInput {
    pub anchor_at: DateTime<Utc>,
    pub recurrence_rule: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct PriceTierInput {
    pub from_accumulated_units: ItemUnitBoundary,
    pub to_accumulated_units: Option<ItemUnitBoundary>,
    pub unit_block_size: ItemUnits,
    pub credit_units: CreditUnits,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreatePriceVersionRequest {
    pub pricing_model: PricingModel,
    pub unit_block_size: Option<ItemUnits>,
    pub credit_units: Option<CreditUnits>,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
    pub accumulation_cycle: Option<AccumulationCycleInput>,
    #[serde(default)]
    pub tiers: Vec<PriceTierInput>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceState {
    Draft,
    Scheduled,
    Active,
    Retired,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PriceVersionResponse {
    pub price_version_id: Uuid,
    pub item_id: Uuid,
    pub pricing_model: PricingModel,
    pub unit_block_size: Option<ItemUnits>,
    pub credit_units: Option<CreditUnits>,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
    pub accumulation_cycle: Option<AccumulationCycleInput>,
    pub tiers: Vec<PriceTierInput>,
    pub state: PriceState,
    pub version: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CatalogScopeItemResponse {
    pub item_id: Uuid,
    pub price_version_id: Uuid,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CatalogScopeResponse {
    pub scope_version: Uuid,
    pub fingerprint: String,
    pub items: Vec<CatalogScopeItemResponse>,
    pub created_at: DateTime<Utc>,
}
