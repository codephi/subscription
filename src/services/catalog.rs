use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    dto::catalog::{
        CatalogScopeResponse, CatalogStatus, CreateItemRequest, CreatePriceVersionRequest,
        CreateProductRequest, ItemResponse, PriceTierInput, PriceVersionResponse, PricingModel,
        ProductResponse, UpdateItemRequest, UpdateProductRequest, UsageModel,
    },
    dto::units::ItemUnits,
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

pub async fn create_product(
    repository: &DatabaseRepository,
    request: CreateProductRequest,
) -> ApiResult<ProductResponse> {
    validate_name("product name", &request.name)?;
    repository.insert_product(&request).await
}

pub async fn get_product(
    repository: &DatabaseRepository,
    product_id: Uuid,
) -> ApiResult<ProductResponse> {
    repository.find_product(product_id).await
}

pub async fn update_product(
    repository: &DatabaseRepository,
    product_id: Uuid,
    request: UpdateProductRequest,
) -> ApiResult<ProductResponse> {
    if let Some(name) = &request.name {
        validate_name("product name", name)?;
    }
    let current = repository.find_product(product_id).await?;
    let usage_model = request.usage_model.unwrap_or(current.usage_model);
    reject_entitlement_publication(usage_model, request.status.unwrap_or(current.status))?;
    validate_product_model_change(repository, product_id, usage_model).await?;
    repository.update_product(product_id, &request).await
}

pub async fn create_item(
    repository: &DatabaseRepository,
    product_id: Uuid,
    request: CreateItemRequest,
) -> ApiResult<ItemResponse> {
    validate_name("item name", &request.name)?;
    let product = repository.find_product(product_id).await?;
    validate_item_shape(product.usage_model, &request)?;
    validate_parent(repository, product_id, request.parent_item_id).await?;
    repository.insert_item(product_id, &request).await
}

pub async fn get_item(repository: &DatabaseRepository, item_id: Uuid) -> ApiResult<ItemResponse> {
    repository.find_item(item_id).await
}

pub async fn update_item(
    repository: &DatabaseRepository,
    item_id: Uuid,
    request: UpdateItemRequest,
) -> ApiResult<ItemResponse> {
    if let Some(name) = &request.name {
        validate_name("item name", name)?;
    }
    let item = repository.find_item(item_id).await?;
    let product = repository.find_product(item.product_id).await?;
    let unit_name = request
        .unit_name
        .as_ref()
        .map(|value| value.as_deref())
        .unwrap_or(item.unit_name.as_deref());
    let quantity_scale = request
        .quantity_scale
        .as_ref()
        .map(|value| value.as_ref().copied())
        .unwrap_or(item.quantity_scale);
    validate_item_unit_values(product.usage_model, unit_name, quantity_scale)?;
    validate_item_parent_update(
        repository,
        item.product_id,
        item.item_id,
        request.parent_item_id.unwrap_or(item.parent_item_id),
    )
    .await?;
    validate_item_activation(repository, &item, request.status).await?;
    repository.update_item(item_id, &request).await
}

pub async fn get_current_catalog_scope(
    repository: &DatabaseRepository,
) -> ApiResult<CatalogScopeResponse> {
    repository.find_current_catalog_scope().await
}

pub async fn create_price_version(
    repository: &DatabaseRepository,
    item_id: Uuid,
    request: CreatePriceVersionRequest,
) -> ApiResult<PriceVersionResponse> {
    validate_metered_item(repository, item_id).await?;
    validate_price(&request)?;
    repository.insert_price_version(item_id, &request).await
}

pub async fn get_price_version(
    repository: &DatabaseRepository,
    price_id: Uuid,
) -> ApiResult<PriceVersionResponse> {
    repository.find_price_version(price_id).await
}

pub async fn publish_price_version(
    repository: &DatabaseRepository,
    price_id: Uuid,
) -> ApiResult<PriceVersionResponse> {
    let price = repository.find_price_version(price_id).await?;
    if price.state == crate::dto::catalog::PriceState::Draft {
        validate_price(&CreatePriceVersionRequest {
            pricing_model: price.pricing_model,
            unit_block_size: price.unit_block_size,
            credit_units: price.credit_units,
            effective_from: price.effective_from,
            effective_until: price.effective_until,
            accumulation_cycle: price.accumulation_cycle,
            tiers: price.tiers,
        })?;
    }
    repository.publish_price_version(price_id).await
}

fn validate_name(label: &str, value: &str) -> ApiResult<()> {
    if (1..=200).contains(&value.trim().len()) {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_catalog_name",
        format!("{label} {value:?} must contain 1 to 200 non-whitespace characters"),
    ))
}

fn reject_entitlement_publication(usage_model: UsageModel, status: CatalogStatus) -> ApiResult<()> {
    if usage_model == UsageModel::EntitlementOnly && status == CatalogStatus::Active {
        return Err(ApiError::unprocessable(
            "usage_model_not_publishable",
            format!(
                "usage model {} cannot be activated because it is unavailable in V1",
                usage_model.as_str()
            ),
        ));
    }
    Ok(())
}

async fn validate_product_model_change(
    repository: &DatabaseRepository,
    product_id: Uuid,
    usage_model: UsageModel,
) -> ApiResult<()> {
    let items = repository.list_items_for_product(product_id).await?;
    if items.iter().all(|item| match usage_model {
        UsageModel::CreditMetered => item.unit_name.is_some() && item.quantity_scale.is_some(),
        UsageModel::EntitlementOnly => item.unit_name.is_none() && item.quantity_scale.is_none(),
    }) {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_product_usage_model",
        format!(
            "product {product_id} has items incompatible with usage model {}",
            usage_model.as_str()
        ),
    ))
}

fn validate_item_shape(model: UsageModel, request: &CreateItemRequest) -> ApiResult<()> {
    let both_present = request.unit_name.is_some() && request.quantity_scale.is_some();
    let both_absent = request.unit_name.is_none() && request.quantity_scale.is_none();
    match model {
        UsageModel::CreditMetered if both_present => Ok(()),
        UsageModel::EntitlementOnly if both_absent => Ok(()),
        _ => Err(ApiError::unprocessable(
            "invalid_item_unit",
            format!(
                "item unit fields do not match usage model {}",
                model.as_str()
            ),
        )),
    }
}

fn validate_item_unit_values(
    model: UsageModel,
    unit_name: Option<&str>,
    quantity_scale: Option<ItemUnits>,
) -> ApiResult<()> {
    let valid = match model {
        UsageModel::CreditMetered => unit_name.is_some() && quantity_scale.is_some(),
        UsageModel::EntitlementOnly => unit_name.is_none() && quantity_scale.is_none(),
    };
    if valid {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_item_unit",
        format!(
            "item unit fields do not match usage model {}",
            model.as_str()
        ),
    ))
}

async fn validate_item_parent_update(
    repository: &DatabaseRepository,
    product_id: Uuid,
    item_id: Uuid,
    parent_id: Option<Uuid>,
) -> ApiResult<()> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    let items = repository.list_items_for_product(product_id).await?;
    let parents = items
        .iter()
        .map(|item| (item.item_id, item.parent_item_id))
        .collect::<std::collections::HashMap<_, _>>();
    let mut visited = HashSet::new();
    let mut ancestor = Some(parent_id);
    while let Some(ancestor_id) = ancestor {
        if ancestor_id == item_id || !visited.insert(ancestor_id) {
            return invalid_item_parent(item_id, parent_id, product_id);
        }
        let Some((_, next)) = parents.get_key_value(&ancestor_id) else {
            return invalid_item_parent(item_id, parent_id, product_id);
        };
        ancestor = *next;
    }
    Ok(())
}

fn invalid_item_parent(item_id: Uuid, parent_id: Uuid, product_id: Uuid) -> ApiResult<()> {
    Err(ApiError::unprocessable(
        "invalid_parent_item",
        format!(
            "parent item {parent_id} must be in product {product_id} and not create a cycle for item {item_id}"
        ),
    ))
}

async fn validate_parent(
    repository: &DatabaseRepository,
    product_id: Uuid,
    parent_id: Option<Uuid>,
) -> ApiResult<()> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    let parent = repository.find_item(parent_id).await?;
    if parent.product_id == product_id {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_parent_item",
        format!("parent item {parent_id} must belong to product {product_id}"),
    ))
}

async fn validate_metered_item(repository: &DatabaseRepository, item_id: Uuid) -> ApiResult<()> {
    let item = repository.find_item(item_id).await?;
    let product = repository.find_product(item.product_id).await?;
    if product.usage_model == UsageModel::CreditMetered {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "price_not_allowed",
        format!(
            "item {item_id} belongs to ENTITLEMENT_ONLY product {}",
            product.product_id
        ),
    ))
}

async fn validate_item_activation(
    repository: &DatabaseRepository,
    item: &ItemResponse,
    status: Option<CatalogStatus>,
) -> ApiResult<()> {
    if status != Some(CatalogStatus::Active) {
        return Ok(());
    }
    let product = repository.find_product(item.product_id).await?;
    if product.usage_model != UsageModel::CreditMetered
        || repository.item_has_published_price(item.item_id).await?
    {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "item_price_required",
        format!(
            "item {} requires a published price before activation",
            item.item_id
        ),
    ))
}

fn validate_price(request: &CreatePriceVersionRequest) -> ApiResult<()> {
    if request
        .effective_until
        .is_some_and(|until| until <= request.effective_from)
    {
        return invalid_price("effective_until must be after effective_from");
    }
    match request.pricing_model {
        PricingModel::Unit => validate_unit_price(request),
        PricingModel::Tiered => validate_tiered_price(request),
    }
}

fn validate_unit_price(request: &CreatePriceVersionRequest) -> ApiResult<()> {
    let credits_positive = request.credit_units.is_some_and(|units| units.value() > 0);
    if request.unit_block_size.is_some()
        && credits_positive
        && request.accumulation_cycle.is_none()
        && request.tiers.is_empty()
    {
        return Ok(());
    }
    invalid_price("unit price requires positive block/credits and forbids cycle/tiers")
}

fn validate_tiered_price(request: &CreatePriceVersionRequest) -> ApiResult<()> {
    if request.unit_block_size.is_some()
        || request.credit_units.is_some()
        || request.tiers.is_empty()
    {
        return invalid_price("tiered price requires tiers and forbids root block/credits");
    }
    validate_cycle(request)?;
    validate_tiers(&request.tiers)
}

fn validate_cycle(request: &CreatePriceVersionRequest) -> ApiResult<()> {
    let Some(cycle) = &request.accumulation_cycle else {
        return Ok(());
    };
    if cycle.anchor_at > request.effective_from
        || !valid_recurrence(&cycle.recurrence_rule)
        || crate::services::calendar::pricing_cycle_bounds(
            cycle.anchor_at,
            &cycle.recurrence_rule,
            request.effective_from,
        )
        .is_err()
    {
        return Err(ApiError::unprocessable(
            "invalid_accumulation_cycle",
            format!(
                "cycle anchor {} and rule {:?} must precede price, use FREQ/INTERVAL and have a representable next UTC boundary",
                cycle.anchor_at, cycle.recurrence_rule
            ),
        ));
    }
    Ok(())
}

fn valid_recurrence(rule: &str) -> bool {
    let mut parts = rule.split(';');
    let frequency = parts.next().and_then(|part| part.strip_prefix("FREQ="));
    let interval = parts.next().and_then(|part| part.strip_prefix("INTERVAL="));
    let valid_frequency = matches!(frequency, Some("DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY"));
    valid_frequency
        && interval
            .and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|value| value > 0)
        && parts.next().is_none()
}

fn validate_tiers(tiers: &[PriceTierInput]) -> ApiResult<()> {
    let mut expected_start = 0_i64;
    for (index, tier) in tiers.iter().enumerate() {
        validate_tier(index, tier, expected_start, index + 1 == tiers.len())?;
        if let Some(end) = tier.to_accumulated_units {
            expected_start = end.value();
        }
    }
    Ok(())
}

fn validate_tier(
    index: usize,
    tier: &PriceTierInput,
    expected_start: i64,
    is_last: bool,
) -> ApiResult<()> {
    let start = tier.from_accumulated_units.value();
    let end = tier.to_accumulated_units.map(|value| value.value());
    let width_is_divisible =
        end.is_none_or(|value| (value - start) % tier.unit_block_size.value() == 0);
    let conversion_fits = end.is_none_or(|value| {
        let blocks = (value - start) / tier.unit_block_size.value();
        blocks.checked_mul(tier.credit_units.value()).is_some()
    });
    if start == expected_start
        && end.is_none_or(|value| value > start)
        && tier.credit_units.value() > 0
        && width_is_divisible
        && conversion_fits
        && (is_last == end.is_none())
    {
        return Ok(());
    }
    invalid_price(&format!(
        "tier {index} must be contiguous, positive, divisible, and only the last tier is open"
    ))
}

fn invalid_price<T>(detail: &str) -> ApiResult<T> {
    Err(ApiError::unprocessable("invalid_price", detail.to_string()))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, Utc};

    use super::validate_price;
    use crate::dto::{
        catalog::{
            AccumulationCycleInput, CreatePriceVersionRequest, PriceTierInput, PricingModel,
        },
        units::{CreditUnits, ItemUnitBoundary, ItemUnits},
    };

    #[test]
    fn unit_price_requires_only_root_conversion() {
        let request = unit_price();
        assert!(validate_price(&request).is_ok());
    }

    #[test]
    fn tiered_price_requires_contiguous_divisible_ranges() {
        let now = Utc::now();
        let request = CreatePriceVersionRequest {
            pricing_model: PricingModel::Tiered,
            unit_block_size: None,
            credit_units: None,
            effective_from: now,
            effective_until: None,
            accumulation_cycle: Some(AccumulationCycleInput {
                anchor_at: now - TimeDelta::days(1),
                recurrence_rule: "FREQ=MONTHLY;INTERVAL=3".to_string(),
            }),
            tiers: vec![tier(0, Some(10), 1, 1), tier(10, None, 10, 3)],
        };
        assert!(validate_price(&request).is_ok());
    }

    #[test]
    fn tiered_price_rejects_gap_and_non_divisible_width() {
        let mut request = tiered_without_cycle();
        request.tiers = vec![tier(0, Some(11), 2, 1), tier(12, None, 1, 1)];
        assert_eq!(
            validate_price(&request).expect_err("invalid tiers").code(),
            "invalid_price"
        );
    }

    fn unit_price() -> CreatePriceVersionRequest {
        CreatePriceVersionRequest {
            pricing_model: PricingModel::Unit,
            unit_block_size: Some(ItemUnits::positive(1000).expect("block")),
            credit_units: Some(CreditUnits::new(100)),
            effective_from: Utc::now(),
            effective_until: None,
            accumulation_cycle: None,
            tiers: Vec::new(),
        }
    }

    fn tiered_without_cycle() -> CreatePriceVersionRequest {
        CreatePriceVersionRequest {
            pricing_model: PricingModel::Tiered,
            unit_block_size: None,
            credit_units: None,
            effective_from: Utc::now(),
            effective_until: None,
            accumulation_cycle: None,
            tiers: Vec::new(),
        }
    }

    fn tier(from: i64, to: Option<i64>, block: i64, credits: i64) -> PriceTierInput {
        PriceTierInput {
            from_accumulated_units: ItemUnitBoundary::non_negative(from).expect("tier start"),
            to_accumulated_units: to
                .map(|value| ItemUnitBoundary::non_negative(value).expect("tier end")),
            unit_block_size: ItemUnits::positive(block).expect("tier block"),
            credit_units: CreditUnits::new(credits),
        }
    }
}
