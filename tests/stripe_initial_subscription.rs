mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use chrono::{Duration, Utc};
use subscription::{
    dto::{
        billing::{CreateBillingConnectionRequest, CreateInitialCollectionRequest},
        plans::{
            AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest,
            CreateSubscriptionPlanRequest, CreateSubscriptionRequest, PlanRecurrence,
            SubscriptionModel,
        },
        units::CreditUnits,
    },
    repositories::{
        billing_confirmation::{ConfirmationResult, ConfirmedBillingWebhook},
        billing_connector::{ConnectorCollectionResult, ConnectorCollectionState},
    },
    services::{billing, plans},
};
use uuid::Uuid;

#[tokio::test]
async fn paid_subscription_checkout_confirmation_grants_initial_cycle_once() {
    let usage = usage_fixture::setup_usage(1, 1, 0).await;
    let account_id = Uuid::new_v4();
    usage_fixture::apply_event(&usage.repository, account_id, "account.created", 1).await;
    usage_fixture::apply_event(&usage.repository, account_id, "account.activated", 2).await;
    let customer_plan_id =
        create_paid_customer_plan(&usage.repository, account_id, usage.product_id).await;
    let binding_id = create_binding(&usage.repository, account_id, customer_plan_id).await;
    let collection = billing::create_initial_collection(
        &usage.repository,
        account_id,
        customer_plan_id,
        "initial-subscription-collection",
        &CreateInitialCollectionRequest {
            payment_method_binding_id: binding_id,
            transaction_id: "initial-subscription-payment".into(),
        },
    )
    .await
    .unwrap();
    let attempt = usage
        .repository
        .begin_collection_attempt(collection.collection_request_id)
        .await
        .unwrap()
        .unwrap();
    usage
        .repository
        .record_collection_result(
            &attempt,
            &ConnectorCollectionResult {
                provider_payment_id: Some("pi_initial_subscription".into()),
                state: ConnectorCollectionState::Pending,
                failure_code: None,
                next_action_url: None,
            },
        )
        .await
        .unwrap();
    let occurred_at = Utc::now();
    let webhook = ConfirmedBillingWebhook {
        provider: "STRIPE".into(),
        provider_event_id: "evt_subscription_checkout".into(),
        event_type: "payment.confirmed".into(),
        payload_sha256: "a".repeat(64),
        collection_request_id: collection.collection_request_id,
        provider_payment_id: "pi_initial_subscription".into(),
        amount_minor: collection.amount_minor,
        currency: collection.currency,
        occurred_at,
    };

    let applied = billing::apply_provider_confirmed_webhook(
        &usage.repository,
        &webhook,
        occurred_at + Duration::days(30),
    )
    .await
    .unwrap();
    let duplicate = billing::apply_provider_confirmed_webhook(
        &usage.repository,
        &webhook,
        occurred_at + Duration::days(30),
    )
    .await
    .unwrap();

    assert_eq!(applied.result, ConfirmationResult::Applied);
    assert_eq!(duplicate.result, ConfirmationResult::Duplicate);
    let balance: i64 = sqlx::query_scalar(
        "SELECT cw.balance_credit_units FROM customer_wallets cw JOIN wallets w USING(wallet_id) WHERE w.customer_id=$1 AND w.wallet_type='CUSTOMER'",
    ).bind(account_id).fetch_one(&usage.pool).await.unwrap();
    assert_eq!(balance, 10);
}

async fn create_paid_customer_plan(
    repository: &subscription::repositories::database::DatabaseRepository,
    account_id: Uuid,
    product_id: Uuid,
) -> Uuid {
    let subscription = plans::create_subscription(
        repository,
        CreateSubscriptionRequest {
            name: "Tasklab paid subscription".into(),
            subscription_model: SubscriptionModel::CreditStrict,
        },
    )
    .await
    .unwrap();
    let plan = plans::create_plan(
        repository,
        subscription.subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Tasklab monthly test".into(),
            commercial_model: CommercialModel::Paid,
            price_amount_minor: Some(100),
            currency: Some("BRL".into()),
            recurrence: PlanRecurrence::Monthly,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: vec!["CARD".into()],
            granted_credit_units: CreditUnits::new(10),
            product_ids: vec![product_id],
        },
    )
    .await
    .unwrap();
    plans::create_customer_plan(
        repository,
        account_id,
        "tasklab-paid-subscription",
        CreateCustomerPlanRequest {
            plan_version_id: plan.plan_version_id,
            transaction_id: "tasklab-paid-subscription".into(),
        },
    )
    .await
    .unwrap()
    .customer_plan_id
}

async fn create_binding(
    repository: &subscription::repositories::database::DatabaseRepository,
    account_id: Uuid,
    customer_plan_id: Uuid,
) -> Uuid {
    let connection = billing::create_billing_connection(
        repository,
        account_id,
        &CreateBillingConnectionRequest {
            provider: "STRIPE".into(),
            external_account_reference: "cus_tasklab_test".into(),
            secret_reference: "env://STRIPE_UNUSED_KEY".into(),
            webhook_secret_reference: "env://STRIPE_UNUSED_WEBHOOK".into(),
        },
    )
    .await
    .unwrap();
    repository
        .create_verified_payment_method_binding(
            account_id,
            connection.billing_connection_id,
            Some(customer_plan_id),
            "pm_tasklab_test",
        )
        .await
        .unwrap()
        .payment_method_binding_id
}
