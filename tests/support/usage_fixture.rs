#![allow(dead_code)]

use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;
use subscription::{
    dto::{
        catalog::{
            AccumulationCycleInput, CatalogStatus, CreateItemRequest, CreatePriceVersionRequest,
            CreateProductRequest, PriceTierInput, PricingModel, UpdateItemRequest,
            UpdateProductRequest, UsageModel,
        },
        events::AccountEventEnvelope,
        plans::{
            AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest,
            CreateSubscriptionPlanRequest, CreateSubscriptionRequest, PlanRecurrence,
            SubscriptionModel,
        },
        units::{CreditUnits, ItemUnits},
        usage::CreateUsageEventRequest,
    },
    repositories::database::DatabaseRepository,
    services::{account_events::process_account_event, catalog, plans},
};
use uuid::Uuid;

use super::support::setup_router_with_options;

pub struct UsageFixture {
    pub router: axum::Router,
    pub pool: PgPool,
    pub repository: DatabaseRepository,
    pub account_id: Uuid,
    pub product_id: Uuid,
    pub item_id: Uuid,
    pub price_id: Uuid,
    pub customer_plan_id: Uuid,
}

pub struct VersionedUsageFixture {
    pub usage: UsageFixture,
    pub old_price_id: Uuid,
    pub new_price_id: Uuid,
    pub boundary: chrono::DateTime<Utc>,
}

pub struct MultiItemUsageFixture {
    pub pool: PgPool,
    pub repository: DatabaseRepository,
    pub account_id: Uuid,
    pub product_id: Uuid,
    pub item_ids: [Uuid; 2],
    pub price_ids: [Uuid; 2],
}

pub async fn setup_usage(block_size: i64, cost: i64, credits: i64) -> UsageFixture {
    setup_priced_usage(
        CreatePriceVersionRequest {
            pricing_model: PricingModel::Unit,
            unit_block_size: Some(ItemUnits::positive(block_size).expect("block")),
            credit_units: Some(CreditUnits::new(cost)),
            effective_from: Utc::now() - Duration::days(1),
            effective_until: None,
            accumulation_cycle: None,
            tiers: Vec::new(),
        },
        credits,
    )
    .await
}

pub async fn setup_tiered_usage(
    tiers: Vec<PriceTierInput>,
    accumulation_cycle: Option<AccumulationCycleInput>,
    credits: i64,
) -> UsageFixture {
    let effective_from = accumulation_cycle
        .as_ref()
        .map_or_else(|| Utc::now() - Duration::days(3), |cycle| cycle.anchor_at);
    setup_priced_usage(
        CreatePriceVersionRequest {
            pricing_model: PricingModel::Tiered,
            unit_block_size: None,
            credit_units: None,
            effective_from,
            effective_until: None,
            accumulation_cycle,
            tiers,
        },
        credits,
    )
    .await
}

pub fn standard_tiers() -> Vec<PriceTierInput> {
    vec![
        price_tier(0, Some(10), 1, 1),
        price_tier(10, Some(20), 2, 1),
        price_tier(20, None, 5, 2),
    ]
}

fn price_tier(from: i64, to: Option<i64>, block: i64, credits: i64) -> PriceTierInput {
    PriceTierInput {
        from_accumulated_units: subscription::dto::units::ItemUnitBoundary::non_negative(from)
            .expect("tier from"),
        to_accumulated_units: to.map(|value| {
            subscription::dto::units::ItemUnitBoundary::non_negative(value).expect("tier to")
        }),
        unit_block_size: ItemUnits::positive(block).expect("tier block"),
        credit_units: CreditUnits::new(credits),
    }
}

pub async fn setup_versioned_usage(credits: i64) -> VersionedUsageFixture {
    let (router, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let product = create_product(&repository).await;
    let item = create_item(&repository, product.product_id).await;
    let boundary = Utc::now() + Duration::seconds(3);
    let old_price_id = create_and_publish_price(
        &repository,
        item.item_id,
        unit_price(10, 10, Utc::now() - Duration::days(1), Some(boundary)),
    )
    .await;
    let new_price_id =
        create_and_publish_price(&repository, item.item_id, unit_price(4, 1, boundary, None)).await;
    activate_catalog(&repository, product.product_id, item.item_id).await;
    let account_id = Uuid::new_v4();
    apply_event(&repository, account_id, "account.created", 1).await;
    apply_event(&repository, account_id, "account.activated", 2).await;
    let customer_plan_id =
        activate_plan(&repository, account_id, product.product_id, credits).await;
    VersionedUsageFixture {
        usage: UsageFixture {
            router,
            pool,
            repository,
            account_id,
            product_id: product.product_id,
            item_id: item.item_id,
            price_id: old_price_id,
            customer_plan_id,
        },
        old_price_id,
        new_price_id,
        boundary,
    }
}

pub async fn setup_two_item_usage(cost: i64, credits: i64) -> MultiItemUsageFixture {
    let (_, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let product = create_product(&repository).await;
    let first = create_item(&repository, product.product_id).await;
    let second = create_item(&repository, product.product_id).await;
    let price_request = || unit_price(1, cost, Utc::now() - Duration::days(1), None);
    let first_price = create_and_publish_price(&repository, first.item_id, price_request()).await;
    let second_price = create_and_publish_price(&repository, second.item_id, price_request()).await;
    activate_item(&repository, first.item_id).await;
    activate_item(&repository, second.item_id).await;
    activate_product(&repository, product.product_id).await;
    let account_id = Uuid::new_v4();
    apply_event(&repository, account_id, "account.created", 1).await;
    apply_event(&repository, account_id, "account.activated", 2).await;
    activate_plan(&repository, account_id, product.product_id, credits).await;
    MultiItemUsageFixture {
        pool,
        repository,
        account_id,
        product_id: product.product_id,
        item_ids: [first.item_id, second.item_id],
        price_ids: [first_price, second_price],
    }
}

async fn setup_priced_usage(
    price_request: CreatePriceVersionRequest,
    credits: i64,
) -> UsageFixture {
    let (router, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let (product_id, item_id, price_id) = create_catalog(&repository, price_request).await;
    let account_id = Uuid::new_v4();
    apply_event(&repository, account_id, "account.created", 1).await;
    apply_event(&repository, account_id, "account.activated", 2).await;
    let customer_plan_id = activate_plan(&repository, account_id, product_id, credits).await;
    UsageFixture {
        router,
        pool,
        repository,
        account_id,
        product_id,
        item_id,
        price_id,
        customer_plan_id,
    }
}

pub fn usage_request(
    fixture: &UsageFixture,
    transaction: &str,
    units: i64,
) -> CreateUsageEventRequest {
    CreateUsageEventRequest {
        transaction_id: transaction.to_string(),
        product_id: fixture.product_id,
        item_id: fixture.item_id,
        item_units: ItemUnits::positive(units).expect("positive usage"),
        expected_price_version_id: Some(fixture.price_id),
        occurred_at: None,
        metadata: Some(json!({"source":"test"})),
    }
}

async fn create_catalog(
    repository: &DatabaseRepository,
    price_request: CreatePriceVersionRequest,
) -> (Uuid, Uuid, Uuid) {
    let product = create_product(repository).await;
    let item = create_item(repository, product.product_id).await;
    let price_id = create_and_publish_price(repository, item.item_id, price_request).await;
    activate_catalog(repository, product.product_id, item.item_id).await;
    (product.product_id, item.item_id, price_id)
}

async fn create_product(
    repository: &DatabaseRepository,
) -> subscription::dto::catalog::ProductResponse {
    catalog::create_product(
        repository,
        CreateProductRequest {
            name: format!("Usage product {}", Uuid::new_v4()),
            description: None,
            usage_model: UsageModel::CreditMetered,
        },
    )
    .await
    .expect("product")
}

async fn create_item(
    repository: &DatabaseRepository,
    product_id: Uuid,
) -> subscription::dto::catalog::ItemResponse {
    catalog::create_item(
        repository,
        product_id,
        CreateItemRequest {
            name: format!("Usage item {}", Uuid::new_v4()),
            parent_item_id: None,
            unit_name: Some("request".to_string()),
            quantity_scale: Some(ItemUnits::positive(1).expect("scale")),
        },
    )
    .await
    .expect("item")
}

async fn create_and_publish_price(
    repository: &DatabaseRepository,
    item_id: Uuid,
    price_request: CreatePriceVersionRequest,
) -> Uuid {
    let price = catalog::create_price_version(repository, item_id, price_request)
        .await
        .expect("price");
    catalog::publish_price_version(repository, price.price_version_id)
        .await
        .expect("publish");
    price.price_version_id
}

async fn activate_catalog(repository: &DatabaseRepository, product_id: Uuid, item_id: Uuid) {
    activate_item(repository, item_id).await;
    activate_product(repository, product_id).await;
}

async fn activate_item(repository: &DatabaseRepository, item_id: Uuid) {
    catalog::update_item(
        repository,
        item_id,
        UpdateItemRequest {
            name: None,
            parent_item_id: None,
            unit_name: None,
            quantity_scale: None,
            status: Some(CatalogStatus::Active),
            expected_version: 1,
        },
    )
    .await
    .expect("activate item");
}

async fn activate_product(repository: &DatabaseRepository, product_id: Uuid) {
    catalog::update_product(
        repository,
        product_id,
        UpdateProductRequest {
            name: None,
            description: None,
            usage_model: None,
            status: Some(CatalogStatus::Active),
            expected_version: 1,
        },
    )
    .await
    .expect("activate product");
}

fn unit_price(
    block_size: i64,
    cost: i64,
    effective_from: chrono::DateTime<Utc>,
    effective_until: Option<chrono::DateTime<Utc>>,
) -> CreatePriceVersionRequest {
    CreatePriceVersionRequest {
        pricing_model: PricingModel::Unit,
        unit_block_size: Some(ItemUnits::positive(block_size).expect("block")),
        credit_units: Some(CreditUnits::new(cost)),
        effective_from,
        effective_until,
        accumulation_cycle: None,
        tiers: Vec::new(),
    }
}

async fn activate_plan(
    repository: &DatabaseRepository,
    account_id: Uuid,
    product_id: Uuid,
    credits: i64,
) -> Uuid {
    let subscription = plans::create_subscription(
        repository,
        CreateSubscriptionRequest {
            name: format!("Usage subscription {}", Uuid::new_v4()),
            subscription_model: SubscriptionModel::CreditStrict,
        },
    )
    .await
    .expect("subscription");
    let plan = plans::create_plan(
        repository,
        subscription.subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Usage plan".to_string(),
            commercial_model: CommercialModel::Free,
            price_amount_minor: None,
            currency: None,
            recurrence: PlanRecurrence::None,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: Vec::new(),
            granted_credit_units: CreditUnits::new(credits),
            product_ids: vec![product_id],
        },
    )
    .await
    .expect("plan");
    plans::create_customer_plan(
        repository,
        account_id,
        &format!("usage-plan-key-{account_id}"),
        CreateCustomerPlanRequest {
            plan_version_id: plan.plan_version_id,
            transaction_id: format!("usage-plan-transaction-{account_id}"),
        },
    )
    .await
    .expect("customer plan")
    .customer_plan_id
}

pub async fn apply_event(
    repository: &DatabaseRepository,
    account_id: Uuid,
    kind: &str,
    sequence: i64,
) {
    let event: AccountEventEnvelope = serde_json::from_value(json!({"event_id":Uuid::new_v4(),"event_type":kind,"schema_version":1,"aggregate_id":account_id,"sequence":sequence,"occurred_at":"2026-09-04T00:00:00Z","account_id":account_id,"correlation_id":Uuid::new_v4(),"causation_id":null,"payload":{"account_id":account_id}})).expect("event");
    process_account_event(repository, event)
        .await
        .expect("apply event");
}
