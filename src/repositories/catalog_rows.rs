use chrono::{DateTime, Utc};
use sqlx::Row;

use crate::{
    dto::{
        catalog::{
            AccumulationCycleInput, CatalogStatus, ItemResponse, PriceState, PriceTierInput,
            PriceVersionResponse, PricingModel, ProductResponse, UsageModel,
        },
        units::{CreditUnits, ItemUnitBoundary, ItemUnits},
    },
    error::{ApiError, ApiResult},
};

pub(super) fn product_from_row(row: &sqlx::postgres::PgRow) -> ApiResult<ProductResponse> {
    Ok(ProductResponse {
        product_id: row.get("product_id"),
        name: row.get("name"),
        description: row.get("description"),
        usage_model: parse_usage_model(row.get("usage_model"))?,
        status: parse_status(row.get("status"))?,
        version: row.get("version"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub(super) fn item_from_row(row: &sqlx::postgres::PgRow) -> ApiResult<ItemResponse> {
    Ok(ItemResponse {
        item_id: row.get("item_id"),
        product_id: row.get("product_id"),
        parent_item_id: row.get("parent_item_id"),
        name: row.get("name"),
        unit_name: row.get("unit_name"),
        quantity_scale: row
            .try_get::<Option<i64>, _>("quantity_scale")?
            .map(ItemUnits::positive)
            .transpose()?,
        status: parse_status(row.get("status"))?,
        version: row.get("version"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub(super) fn price_from_parts(
    row: &sqlx::postgres::PgRow,
    tiers: Vec<PriceTierInput>,
) -> ApiResult<PriceVersionResponse> {
    let anchor: Option<DateTime<Utc>> = row.try_get("accumulation_anchor_at")?;
    let rule: Option<String> = row.try_get("accumulation_recurrence_rule")?;
    Ok(PriceVersionResponse {
        price_version_id: row.get("price_version_id"),
        item_id: row.get("item_id"),
        pricing_model: parse_pricing_model(row.get("pricing_model"))?,
        unit_block_size: row
            .try_get::<Option<i64>, _>("unit_block_size")?
            .map(ItemUnits::positive)
            .transpose()?,
        credit_units: row
            .try_get::<Option<i64>, _>("credit_units")?
            .map(CreditUnits::new),
        effective_from: row.get("effective_from"),
        effective_until: row.get("effective_until"),
        accumulation_cycle: anchor.zip(rule).map(|(anchor_at, recurrence_rule)| {
            AccumulationCycleInput {
                anchor_at,
                recurrence_rule,
            }
        }),
        tiers,
        state: parse_price_state(row.get("state"))?,
        version: row.get("version"),
        created_at: row.get("created_at"),
    })
}

pub(super) fn tier_from_row(row: &sqlx::postgres::PgRow) -> ApiResult<PriceTierInput> {
    Ok(PriceTierInput {
        from_accumulated_units: ItemUnitBoundary::non_negative(row.get("from_accumulated_units"))?,
        to_accumulated_units: row
            .try_get::<Option<i64>, _>("to_accumulated_units")?
            .map(ItemUnitBoundary::non_negative)
            .transpose()?,
        unit_block_size: ItemUnits::positive(row.get("unit_block_size"))?,
        credit_units: CreditUnits::new(row.get("credit_units")),
    })
}

fn parse_usage_model(value: &str) -> ApiResult<UsageModel> {
    match value {
        "CREDIT_METERED" => Ok(UsageModel::CreditMetered),
        "ENTITLEMENT_ONLY" => Ok(UsageModel::EntitlementOnly),
        _ => Err(ApiError::unexpected(format!(
            "unknown usage_model {value:?}"
        ))),
    }
}

fn parse_status(value: &str) -> ApiResult<CatalogStatus> {
    match value {
        "ACTIVE" => Ok(CatalogStatus::Active),
        "INACTIVE" => Ok(CatalogStatus::Inactive),
        "ARCHIVED" => Ok(CatalogStatus::Archived),
        _ => Err(ApiError::unexpected(format!(
            "unknown catalog status {value:?}"
        ))),
    }
}

fn parse_pricing_model(value: &str) -> ApiResult<PricingModel> {
    match value {
        "unit" => Ok(PricingModel::Unit),
        "tiered" => Ok(PricingModel::Tiered),
        _ => Err(ApiError::unexpected(format!(
            "unknown pricing_model {value:?}"
        ))),
    }
}

fn parse_price_state(value: &str) -> ApiResult<PriceState> {
    match value {
        "DRAFT" => Ok(PriceState::Draft),
        "SCHEDULED" => Ok(PriceState::Scheduled),
        "ACTIVE" => Ok(PriceState::Active),
        "RETIRED" => Ok(PriceState::Retired),
        _ => Err(ApiError::unexpected(format!(
            "unknown price state {value:?}"
        ))),
    }
}
