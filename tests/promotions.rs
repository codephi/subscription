#[path = "support/credit_fixture.rs"]
mod credit_fixture;
mod support;

use chrono::{Duration, Utc};
use credit_fixture::CreditFixture;
use subscription::dto::{
    billing::CreateBillingConnectionRequest,
    catalog::{
        CatalogStatus, CreateItemRequest, CreatePriceVersionRequest, CreateProductRequest,
        PricingModel, UpdateItemRequest, UpdateProductRequest, UsageModel,
    },
    checkouts::{CheckoutKind, CreateCheckoutRequest},
    credits::UpdateAccountBillingConfigRequest,
    events::AccountEventEnvelope,
    plans::{
        AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest, CreateSubscriptionPlanRequest,
        CreateSubscriptionRequest, PlanRecurrence, SubscriptionModel,
    },
    units::{CreditUnits, ItemUnits},
};
use subscription::{
    dto::promotions::{CreateCouponRequest, CreateVoucherRequest, RedeemVoucherRequest},
    repositories::database::DatabaseRepository,
    services::{account_events, billing, billing_checkout, catalog, credits, plans, promotions},
};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simultaneous_redemptions_respect_the_account_limit_and_credit_once() {
    let fixture = CreditFixture::new().await;
    let voucher = promotions::create_voucher(
        &fixture.repository,
        CreateVoucherRequest {
            code: " first-use ".into(),
            name: "First use voucher".into(),
            description: None,
            credit_units: CreditUnits::new(250),
            valid_from: None,
            valid_until: None,
            max_total_uses: Some(20),
            max_uses_per_account: Some(1),
        },
    )
    .await
    .expect("create voucher");
    let voucher_id = voucher.promotion_id;
    let first_repo = fixture.repository.clone();
    let second_repo = fixture.repository.clone();
    let account_id = fixture.account_id;
    let (first, second) = tokio::join!(
        promotions::redeem_voucher(
            &first_repo,
            account_id,
            "redeem-first",
            redemption(voucher_id, "transaction-first"),
        ),
        promotions::redeem_voucher(
            &second_repo,
            account_id,
            "redeem-second",
            redemption(voucher_id, "transaction-second"),
        )
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let balance: i64 = sqlx::query_scalar(
        "SELECT cw.balance_credit_units FROM customer_wallets cw JOIN wallets w USING(wallet_id) WHERE w.customer_id=$1",
    )
    .bind(account_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("account balance");
    assert_eq!(balance, 250);
    let uses: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM voucher_redemptions WHERE voucher_id=$1 AND account_id=$2",
    )
    .bind(voucher_id)
    .bind(account_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("voucher redemption count");
    assert_eq!(uses, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_discount_completes_initial_checkout_without_a_payment_request() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let product_id = create_active_product(&repository).await;
    let account_id = Uuid::new_v4();
    for (event, sequence) in [("account.created", 1), ("account.activated", 2)] {
        let envelope: AccountEventEnvelope = serde_json::from_value(serde_json::json!({
            "event_id": Uuid::new_v4(), "event_type": event, "schema_version": 1,
            "aggregate_id": account_id, "account_id": account_id, "sequence": sequence,
            "occurred_at": Utc::now(), "correlation_id": Uuid::new_v4(),
            "payload": {"account_id": account_id}
        }))
        .unwrap();
        account_events::process_account_event(&repository, envelope)
            .await
            .unwrap();
    }
    credits::update_billing_config(
        &repository,
        account_id,
        UpdateAccountBillingConfigRequest {
            direct_credit_enabled: false,
            recurring_credit_enabled: true,
            expected_version: 1,
        },
    )
    .await
    .expect("enable recurring cycle grants");
    let subscription = plans::create_subscription(
        &repository,
        CreateSubscriptionRequest {
            name: "Promotion initial checkout".into(),
            subscription_model: SubscriptionModel::CreditStrict,
        },
    )
    .await
    .expect("subscription");
    let plan = plans::create_plan(
        &repository,
        subscription.subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Annual membership".into(),
            commercial_model: CommercialModel::Paid,
            price_amount_minor: Some(2500),
            currency: Some("USD".into()),
            recurrence: PlanRecurrence::Monthly,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: vec!["CARD".into()],
            granted_credit_units: CreditUnits::new(120),
            product_ids: vec![product_id],
        },
    )
    .await
    .expect("paid plan");
    let customer_plan = plans::create_customer_plan(
        &repository,
        account_id,
        "admission-1",
        CreateCustomerPlanRequest {
            plan_version_id: plan.plan_version_id,
            transaction_id: "admission-transaction-1".into(),
        },
    )
    .await
    .expect("initial customer plan");
    let coupon = promotions::create_coupon(
        &repository,
        CreateCouponRequest {
            code: "FREE100".into(),
            name: "Free first cycle".into(),
            description: None,
            discount_kind: "PERCENTAGE".into(),
            discount_value: 10_000,
            currency: None,
            applies_to_initial: true,
            applies_to_on_demand: false,
            valid_from: None,
            valid_until: None,
            max_total_uses: Some(1),
            max_uses_per_account: Some(1),
        },
    )
    .await
    .expect("full discount coupon");
    let request = CreateCheckoutRequest {
        customer_plan_id: customer_plan.customer_plan_id,
        checkout_kind: CheckoutKind::Initial,
        on_demand_plan_id: None,
        target_plan_version_id: None,
        quantity: None,
        success_url: Some("https://tasklab.example/success".into()),
        cancel_url: Some("https://tasklab.example/cancel".into()),
        transaction_id: "free-checkout-1".into(),
        coupon_code: Some("FREE100".into()),
        payment_method_binding_id: None,
        save_payment_method: false,
        payment_method_name: None,
    };
    let checkout = billing_checkout::create(
        &repository,
        None,
        account_id,
        "checkout-free-1",
        request.clone(),
    )
    .await
    .expect("free checkout");
    assert_eq!(checkout.status, "COMPLETED");
    assert!(!checkout.payment_required);
    assert_eq!(checkout.amount_minor, Some(0));
    assert_eq!(checkout.base_amount_minor, Some(2500));
    assert_eq!(checkout.collection_request_id, None);
    assert_eq!(checkout.granted_credit_units, Some(120));
    let retry = billing_checkout::create(&repository, None, account_id, "checkout-free-1", request)
        .await
        .expect("checkout retry");
    assert_eq!(retry, checkout);
    let uses: i64 = sqlx::query_scalar("SELECT count(*) FROM coupon_checkout_reservations WHERE coupon_id=$1 AND status='COMPLETED'")
        .bind(coupon.promotion_id).fetch_one(&pool).await.expect("completed coupon use");
    assert_eq!(uses, 1);
    let cycles: i64 =
        sqlx::query_scalar("SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1")
            .bind(customer_plan.customer_plan_id)
            .fetch_one(&pool)
            .await
            .expect("initial plan cycle");
    assert_eq!(cycles, 1);
    let balance: i64 = sqlx::query_scalar(
        "SELECT cw.balance_credit_units FROM customer_wallets cw JOIN wallets w USING(wallet_id) WHERE w.customer_id=$1",
    )
    .bind(account_id)
    .fetch_one(&pool)
    .await
    .expect("granted cycle credits");
    assert_eq!(balance, 120);
    drop(router);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paid_coupon_checkout_reserves_capacity_and_terminal_failure_releases_it() {
    let (_router, pool) = support::setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let product_id = create_active_product(&repository).await;
    let account_id = Uuid::new_v4();
    activate_account(&repository, account_id).await;
    credits::update_billing_config(
        &repository,
        account_id,
        UpdateAccountBillingConfigRequest {
            direct_credit_enabled: false,
            recurring_credit_enabled: true,
            expected_version: 1,
        },
    )
    .await
    .expect("enable recurring cycle grants");
    let plan = create_paid_plan(&repository, product_id).await;
    let customer_plan = plans::create_customer_plan(
        &repository,
        account_id,
        "paid-coupon-admission",
        CreateCustomerPlanRequest {
            plan_version_id: plan,
            transaction_id: "paid-coupon-plan".into(),
        },
    )
    .await
    .expect("initial customer plan");
    let connection = billing::create_billing_connection(
        &repository,
        account_id,
        &CreateBillingConnectionRequest {
            provider: "STRIPE".into(),
            external_account_reference: "cus_coupon_test".into(),
            secret_reference: "env://STRIPE_SECRET_KEY".into(),
            webhook_secret_reference: "env://STRIPE_WEBHOOK_SECRET".into(),
        },
    )
    .await
    .expect("billing connection");
    let binding = repository
        .create_verified_payment_method_binding(
            account_id,
            connection.billing_connection_id,
            Some(customer_plan.customer_plan_id),
            "pm_coupon_test",
        )
        .await
        .expect("payment binding");
    let coupon = promotions::create_coupon(
        &repository,
        CreateCouponRequest {
            code: "HALFOFF".into(),
            name: "Half off".into(),
            description: None,
            discount_kind: "PERCENTAGE".into(),
            discount_value: 5_000,
            currency: None,
            applies_to_initial: true,
            applies_to_on_demand: false,
            valid_from: None,
            valid_until: None,
            max_total_uses: Some(1),
            max_uses_per_account: Some(1),
        },
    )
    .await
    .expect("half off coupon");
    let checkout = billing_checkout::create(
        &repository,
        None,
        account_id,
        "paid-coupon-checkout-key",
        CreateCheckoutRequest {
            customer_plan_id: customer_plan.customer_plan_id,
            checkout_kind: CheckoutKind::Initial,
            on_demand_plan_id: None,
            target_plan_version_id: None,
            quantity: None,
            success_url: None,
            cancel_url: None,
            transaction_id: "paid-coupon-checkout-tx".into(),
            coupon_code: Some("HALFOFF".into()),
            payment_method_binding_id: Some(binding.payment_method_binding_id),
            save_payment_method: false,
            payment_method_name: None,
        },
    )
    .await
    .expect("paid coupon checkout");
    assert!(checkout.payment_required);
    assert_eq!(checkout.base_amount_minor, Some(2_500));
    assert_eq!(checkout.discount_amount_minor, 1_250);
    assert_eq!(checkout.amount_minor, Some(1_250));
    let collection_id = checkout.collection_request_id.expect("collection request");
    let snapshot: (i64, i64, i64) = sqlx::query_as(
        "SELECT amount_minor,base_amount_minor,discount_amount_minor FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(collection_id)
    .fetch_one(&pool)
    .await
    .expect("immutable discount snapshot");
    assert_eq!(snapshot, (1_250, 2_500, 1_250));
    let reserved: (i64, i64) = sqlx::query_as(
        "SELECT reserved_uses,completed_uses FROM promotion_usage_counters WHERE promotion_kind='COUPON' AND promotion_id=$1 AND account_id=$2",
    )
    .bind(coupon.promotion_id)
    .bind(account_id)
    .fetch_one(&pool)
    .await
    .expect("reserved coupon use");
    assert_eq!(reserved, (1, 0));
    sqlx::query("UPDATE collection_requests SET status='EXHAUSTED',terminal_reason='test_terminal_failure' WHERE collection_request_id=$1")
        .bind(collection_id)
        .execute(&pool)
        .await
        .expect("terminal failure");
    let released: (String, i64, i64) = sqlx::query_as(
        "SELECT r.status,c.reserved_uses,c.completed_uses FROM coupon_checkout_reservations r JOIN promotion_usage_counters c ON c.promotion_kind='COUPON' AND c.promotion_id=r.coupon_id AND c.account_id=r.account_id WHERE r.checkout_id=$1",
    )
    .bind(checkout.checkout_id)
    .fetch_one(&pool)
    .await
    .expect("released coupon use");
    assert_eq!(released, ("RELEASED".into(), 0, 0));
}

fn redemption(voucher_id: uuid::Uuid, transaction_id: &str) -> RedeemVoucherRequest {
    RedeemVoucherRequest {
        voucher_id: Some(voucher_id),
        code: None,
        transaction_id: transaction_id.into(),
        description: None,
    }
}

async fn create_active_product(repository: &DatabaseRepository) -> Uuid {
    let product = catalog::create_product(
        repository,
        CreateProductRequest {
            name: format!("Coupon checkout product {}", Uuid::new_v4()),
            description: None,
            usage_model: UsageModel::CreditMetered,
        },
    )
    .await
    .expect("product");
    let item = catalog::create_item(
        repository,
        product.product_id,
        CreateItemRequest {
            name: "Pipeline run".into(),
            parent_item_id: None,
            unit_name: Some("run".into()),
            quantity_scale: Some(ItemUnits::positive(1).expect("unit scale")),
        },
    )
    .await
    .expect("item");
    let price = catalog::create_price_version(
        repository,
        item.item_id,
        CreatePriceVersionRequest {
            pricing_model: PricingModel::Unit,
            unit_block_size: Some(ItemUnits::positive(1).unwrap()),
            credit_units: Some(CreditUnits::new(1)),
            effective_from: Utc::now() - Duration::days(1),
            effective_until: None,
            accumulation_cycle: None,
            tiers: vec![],
        },
    )
    .await
    .expect("price");
    catalog::publish_price_version(repository, price.price_version_id)
        .await
        .expect("publish price");
    catalog::update_item(
        repository,
        item.item_id,
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
    catalog::update_product(
        repository,
        product.product_id,
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
    product.product_id
}

async fn activate_account(repository: &DatabaseRepository, account_id: Uuid) {
    for (event, sequence) in [("account.created", 1), ("account.activated", 2)] {
        let envelope: AccountEventEnvelope = serde_json::from_value(serde_json::json!({
            "event_id": Uuid::new_v4(), "event_type": event, "schema_version": 1,
            "aggregate_id": account_id, "account_id": account_id, "sequence": sequence,
            "occurred_at": Utc::now(), "correlation_id": Uuid::new_v4(),
            "payload": {"account_id": account_id}
        }))
        .unwrap();
        account_events::process_account_event(repository, envelope)
            .await
            .unwrap();
    }
}

async fn create_paid_plan(repository: &DatabaseRepository, product_id: Uuid) -> Uuid {
    let subscription = plans::create_subscription(
        repository,
        CreateSubscriptionRequest {
            name: "Paid coupon checkout".into(),
            subscription_model: SubscriptionModel::CreditStrict,
        },
    )
    .await
    .expect("subscription");
    plans::create_plan(
        repository,
        subscription.subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Monthly membership".into(),
            commercial_model: CommercialModel::Paid,
            price_amount_minor: Some(2_500),
            currency: Some("USD".into()),
            recurrence: PlanRecurrence::Monthly,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: vec!["CARD".into()],
            granted_credit_units: CreditUnits::new(120),
            product_ids: vec![product_id],
        },
    )
    .await
    .expect("paid plan")
    .plan_version_id
}
