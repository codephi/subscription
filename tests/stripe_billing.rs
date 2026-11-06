mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use std::sync::{Arc, Mutex};

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subscription::{
    dto::{
        admin_queries::CreateAccountRequest,
        billing::{CreateBillingConnectionRequest, CreateInitialCollectionRequest},
        events::{AccountEventEnvelope, AccountEventPayload, AccountEventType},
        plans::{
            AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest,
            CreateSubscriptionPlanRequest, CreateSubscriptionRequest, PlanRecurrence,
            SubscriptionModel,
        },
        units::CreditUnits,
    },
    repositories::{
        billing_connector::{
            BillingConnector, BillingPaymentMethod, CollectionCommand, ConnectorCollectionResult,
            ConnectorCollectionState, HostedPaymentSessionCommand, SetupSessionCommand,
        },
        stripe::StripeConnector,
    },
    services::{
        account_events::process_account_event, admin_queries, billing, billing_integrations, plans,
        stripe_webhooks,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use uuid::Uuid;

use usage_fixture::setup_usage;

struct EnvironmentValue {
    name: String,
    previous: Option<std::ffi::OsString>,
}

impl EnvironmentValue {
    fn set(name: &str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self {
            name: name.into(),
            previous,
        }
    }
}

impl Drop for EnvironmentValue {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(&self.name, previous);
        } else {
            std::env::remove_var(&self.name);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_stripe_credentials_are_encrypted_and_webhook_setup_activates_connection() {
    let fixture = setup_usage(1, 1, 0).await;
    let _key = EnvironmentValue::set(
        "BILLING_CREDENTIAL_ENCRYPTION_KEY",
        "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=",
    );
    let repository =
        subscription::repositories::database::DatabaseRepository::new(fixture.repository.pool())
            .with_credential_vault()
            .expect("configure credential vault");
    let integration = repository
        .create_stripe_integration(
            fixture.account_id,
            "acct_managed_test",
            "TEST",
            None,
            "sk_test_managed_secret",
        )
        .await
        .unwrap();
    assert_eq!(integration.status, "PENDING_SETUP");
    let stored: String = sqlx::query_scalar(
        "SELECT secret_reference FROM billing_connections WHERE billing_connection_id=$1",
    )
    .bind(integration.billing_connection_id)
    .fetch_one(&repository.pool())
    .await
    .unwrap();
    assert!(!stored.contains("sk_test_managed_secret"));
    assert_eq!(
        repository
            .credential_vault()
            .unwrap()
            .open(
                fixture.account_id,
                integration.billing_connection_id,
                "stripe_api",
                &stored,
            )
            .unwrap(),
        "sk_test_managed_secret"
    );

    let configured = repository
        .update_stripe_integration(
            fixture.account_id,
            integration.billing_connection_id,
            integration.configuration_version,
            None,
            Some("whsec_managed_test"),
        )
        .await
        .unwrap();
    assert_eq!(configured.status, "ACTIVE");
    assert!(configured.webhook_secret_configured);
    let summaries = repository
        .list_integrations(fixture.account_id)
        .await
        .unwrap();
    let public_summary = serde_json::to_string(&summaries).unwrap();
    assert!(!public_summary.contains("whsec_managed_test"));
    assert!(!public_summary.contains("sk_test_managed_secret"));

    let delayed_account = admin_queries::create_account(
        &repository,
        CreateAccountRequest {
            actor_reference: "stripe-billing-test".into(),
        },
    )
    .await
    .unwrap();
    let defaults = repository
        .save_default_stripe_credentials(
            0,
            "TEST",
            "acct_default_test",
            Some("sk_test_default_secret"),
            Some("whsec_default_secret"),
        )
        .await
        .unwrap();
    assert!(defaults.configured);
    assert!(defaults.webhook_secret_configured);
    let stored_default: String = sqlx::query_scalar(
        "SELECT api_secret_reference FROM billing_default_stripe_credentials WHERE singleton_id=1",
    )
    .fetch_one(&repository.pool())
    .await
    .unwrap();
    assert!(!stored_default.contains("sk_test_default_secret"));
    let loaded = repository
        .load_default_stripe_secrets()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.api_secret, "sk_test_default_secret");
    assert_eq!(
        loaded.webhook_secret.as_deref(),
        Some("whsec_default_secret")
    );
    let recovered_connection = billing_integrations::active_or_provision_default_stripe(
        &repository,
        delayed_account.account_id,
    )
    .await
    .unwrap();
    let recovered = repository
        .get_integration(delayed_account.account_id, recovered_connection)
        .await
        .unwrap();
    assert_eq!(recovered.status, "ACTIVE");
    repository
        .provision_account_default_stripe(fixture.account_id, &loaded)
        .await
        .unwrap();
    repository
        .provision_account_default_stripe(fixture.account_id, &loaded)
        .await
        .unwrap();
    let provisioned = repository
        .find_test_stripe_integration(fixture.account_id, "acct_default_test")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(provisioned.status, "ACTIVE");
    let defaults_public = serde_json::to_string(&defaults).unwrap();
    assert!(!defaults_public.contains("sk_test_default_secret"));
    assert!(!defaults_public.contains("whsec_default_secret"));

    let new_account = Uuid::new_v4();
    process_account_event(
        &repository,
        AccountEventEnvelope {
            event_id: Uuid::new_v4(),
            event_type: AccountEventType::Created,
            schema_version: 1,
            aggregate_id: new_account,
            sequence: 1,
            occurred_at: Utc::now(),
            account_id: new_account,
            correlation_id: Uuid::new_v4(),
            causation_id: None,
            payload: AccountEventPayload {
                account_id: new_account,
            },
        },
    )
    .await
    .unwrap();
    let new_account_integration = repository
        .find_test_stripe_integration(new_account, "acct_default_test")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(new_account_integration.status, "ACTIVE");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn payment_setup_reference_resolves_inside_subscription_without_client_integration_id() {
    let fixture = setup_usage(1, 1, 0).await;
    let repository = &fixture.repository;
    let connection = repository
        .create_billing_connection(
            fixture.account_id,
            &CreateBillingConnectionRequest {
                provider: "STRIPE".into(),
                external_account_reference: "acct_setup_reference_test".into(),
                secret_reference: "env://STRIPE_TEST_SECRET".into(),
                webhook_secret_reference: "env://STRIPE_TEST_WEBHOOK".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        repository
            .active_stripe_billing_connection(fixture.account_id)
            .await
            .unwrap(),
        connection.billing_connection_id
    );

    let setup_reference = Uuid::new_v4();
    repository
        .record_payment_method_setup_session(
            setup_reference,
            fixture.account_id,
            connection.billing_connection_id,
            fixture.customer_plan_id,
            "cs_test_provider_session",
            None,
        )
        .await
        .unwrap();
    let setup = repository
        .find_payment_method_setup_session(
            fixture.account_id,
            fixture.customer_plan_id,
            setup_reference,
        )
        .await
        .unwrap();
    assert_eq!(
        setup.billing_connection_id,
        connection.billing_connection_id
    );
    assert_eq!(setup.provider_setup_id, "cs_test_provider_session");
    assert!(repository
        .find_payment_method_setup_session(fixture.account_id, Uuid::new_v4(), setup_reference,)
        .await
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn payment_setup_requires_subscription_to_resolve_one_active_integration() {
    let fixture = setup_usage(1, 1, 0).await;
    for account in ["acct_setup_first", "acct_setup_second"] {
        fixture
            .repository
            .create_billing_connection(
                fixture.account_id,
                &CreateBillingConnectionRequest {
                    provider: "STRIPE".into(),
                    external_account_reference: account.into(),
                    secret_reference: "env://STRIPE_TEST_SECRET".into(),
                    webhook_secret_reference: "env://STRIPE_TEST_WEBHOOK".into(),
                },
            )
            .await
            .unwrap();
    }
    let error = fixture
        .repository
        .active_stripe_billing_connection(fixture.account_id)
        .await
        .expect_err("ambiguous integration must stay inside Subscription policy");
    assert_eq!(error.code(), "billing_connection_ambiguous");
}

struct FakeStripeServer {
    address: std::net::SocketAddr,
    received: Arc<Mutex<String>>,
    task: tokio::task::JoinHandle<()>,
}

impl FakeStripeServer {
    async fn responding_with(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let received = Arc::new(Mutex::new(String::new()));
        let server_received = Arc::clone(&received);
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0_u8; 8192];
            let read = socket.read(&mut bytes).await.unwrap();
            *server_received.lock().unwrap() = String::from_utf8_lossy(&bytes[..read]).into_owned();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        Self {
            address,
            received,
            task,
        }
    }

    async fn finish(self) -> String {
        self.task.await.unwrap();
        Arc::try_unwrap(self.received)
            .unwrap()
            .into_inner()
            .unwrap()
    }

    fn api_base(&self) -> String {
        format!("http://{}", self.address)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stripe_webhooks_converge_without_duplicate_effects() {
    let fixture = setup_usage(1, 1, 0).await;
    let subscription = plans::create_subscription(
        &fixture.repository,
        CreateSubscriptionRequest {
            name: "Stripe subscription".into(),
            subscription_model: SubscriptionModel::CreditStrict,
        },
    )
    .await
    .unwrap();
    let plan = plans::create_plan(
        &fixture.repository,
        subscription.subscription_id,
        CreateSubscriptionPlanRequest {
            admission_policy_version_id: None,
            name: "Stripe paid plan".into(),
            commercial_model: CommercialModel::Paid,
            price_amount_minor: Some(1_500),
            currency: Some("BRL".into()),
            recurrence: PlanRecurrence::Monthly,
            admission_policy: AdmissionPolicy::Open,
            accepted_payment_methods: vec!["CARD".into()],
            granted_credit_units: CreditUnits::new(100),
            product_ids: vec![fixture.product_id],
        },
    )
    .await
    .unwrap();
    let customer_plan = plans::create_customer_plan(
        &fixture.repository,
        fixture.account_id,
        "stripe-plan-key",
        CreateCustomerPlanRequest {
            plan_version_id: plan.plan_version_id,
            transaction_id: "stripe-plan-transaction".into(),
        },
    )
    .await
    .unwrap();
    let secret_variable = format!("STRIPE_WEBHOOK_SECRET_{}", fixture.account_id.simple());
    std::env::set_var(&secret_variable, "whsec_integration");
    let connection = billing::create_billing_connection(
        &fixture.repository,
        fixture.account_id,
        &CreateBillingConnectionRequest {
            provider: "STRIPE".into(),
            external_account_reference: "cus_webhook".into(),
            secret_reference: "env://STRIPE_SECRET_KEY_UNUSED".into(),
            webhook_secret_reference: format!("env://{secret_variable}"),
        },
    )
    .await
    .unwrap();
    let binding = fixture
        .repository
        .create_verified_payment_method_binding(
            fixture.account_id,
            connection.billing_connection_id,
            Some(customer_plan.customer_plan_id),
            "pm_webhook",
        )
        .await
        .unwrap();
    let collection = billing::create_initial_collection(
        &fixture.repository,
        fixture.account_id,
        customer_plan.customer_plan_id,
        "stripe-collection-key",
        &CreateInitialCollectionRequest {
            payment_method_binding_id: binding.payment_method_binding_id,
            transaction_id: "stripe-collection-transaction".into(),
        },
    )
    .await
    .unwrap();
    let attempt = fixture
        .repository
        .begin_collection_attempt(collection.collection_request_id)
        .await
        .unwrap()
        .unwrap();
    fixture
        .repository
        .record_collection_result(
            &attempt,
            &ConnectorCollectionResult {
                provider_payment_id: Some("pi_webhook".into()),
                state: ConnectorCollectionState::Pending,
                failure_code: None,
                next_action_url: None,
            },
        )
        .await
        .unwrap();
    let timestamp = Utc::now().timestamp();
    let payload = format!(
        r#"{{"id":"evt_webhook","type":"payment_intent.succeeded","created":{timestamp},"data":{{"object":{{"id":"pi_webhook","amount":1500,"currency":"brl","metadata":{{"collection_request_id":"{}"}}}}}}}}"#,
        collection.collection_request_id
    );
    let signature = stripe_signature(timestamp, payload.as_bytes(), "whsec_integration");
    let invalid = stripe_webhooks::process_stripe_webhook(
        &fixture.repository,
        connection.billing_connection_id,
        "t=0,v1=invalid",
        payload.as_bytes(),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        invalid.code(),
        "stripe_signature_expired" | "stripe_signature_invalid"
    ));
    let action_payload = payload
        .replace("evt_webhook", "evt_action")
        .replace("payment_intent.succeeded", "payment_intent.requires_action");
    let action_signature =
        stripe_signature(timestamp, action_payload.as_bytes(), "whsec_integration");
    let action = stripe_webhooks::process_stripe_webhook(
        &fixture.repository,
        connection.billing_connection_id,
        &action_signature,
        action_payload.as_bytes(),
    )
    .await
    .unwrap();
    assert_eq!(action.result, "APPLIED");
    let first = stripe_webhooks::process_stripe_webhook(
        &fixture.repository,
        connection.billing_connection_id,
        &signature,
        payload.as_bytes(),
    )
    .await
    .unwrap();
    let duplicate = stripe_webhooks::process_stripe_webhook(
        &fixture.repository,
        connection.billing_connection_id,
        &signature,
        payload.as_bytes(),
    )
    .await
    .unwrap();
    let late_failure_payload = payload
        .replace("evt_webhook", "evt_late_failure")
        .replace("payment_intent.succeeded", "payment_intent.payment_failed");
    let late_failure_signature = stripe_signature(
        timestamp,
        late_failure_payload.as_bytes(),
        "whsec_integration",
    );
    let late_failure = stripe_webhooks::process_stripe_webhook(
        &fixture.repository,
        connection.billing_connection_id,
        &late_failure_signature,
        late_failure_payload.as_bytes(),
    )
    .await
    .unwrap();
    std::env::remove_var(secret_variable);
    assert_eq!(first.result, "APPLIED");
    assert_eq!(duplicate.result, "DUPLICATE");
    assert_eq!(late_failure.result, "REJECTED");
    let effects: (i64, i64, i64) = sqlx::query_as(
        "SELECT cw.balance_credit_units, \
         (SELECT count(*) FROM customer_plan_cycles WHERE customer_plan_id=$1), \
         (SELECT count(*) FROM billing_webhook_inbox WHERE provider_event_id IN ('evt_action','evt_webhook','evt_late_failure')) \
         FROM customer_wallets cw JOIN wallets w USING(wallet_id) \
         WHERE w.customer_id=$2 AND w.wallet_type='CUSTOMER'",
    )
    .bind(customer_plan.customer_plan_id)
    .bind(fixture.account_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(effects, (100, 1, 3));
}

fn stripe_signature(timestamp: i64, payload: &[u8], secret: &str) -> String {
    let signed = [timestamp.to_string().as_bytes(), b".", payload].concat();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(&signed);
    let digest: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("t={timestamp},v1={digest}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stripe_connection_and_tokenized_binding_never_expose_secret_or_card_data() {
    let fixture = setup_usage(1, 1, 0).await;
    let rejected = billing::create_billing_connection(
        &fixture.repository,
        fixture.account_id,
        &CreateBillingConnectionRequest {
            provider: "STRIPE".into(),
            external_account_reference: "cus_test".into(),
            secret_reference: "raw_test_key".into(),
            webhook_secret_reference: "raw_webhook_secret".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(rejected.code(), "invalid_secret_reference");
    let connection = billing::create_billing_connection(
        &fixture.repository,
        fixture.account_id,
        &CreateBillingConnectionRequest {
            provider: "STRIPE".into(),
            external_account_reference: "cus_test".into(),
            secret_reference: "env://STRIPE_SECRET_KEY".into(),
            webhook_secret_reference: "env://STRIPE_WEBHOOK_SECRET".into(),
        },
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&connection).unwrap();
    assert!(!serialized.contains("SECRET"));
    let binding = fixture
        .repository
        .create_verified_payment_method_binding_with_name(
            fixture.account_id,
            connection.billing_connection_id,
            None,
            "pm_tokenized_test",
            Some("Cartão •••• 4242"),
        )
        .await
        .unwrap();
    let bindings = billing::list_payment_method_bindings(&fixture.repository, fixture.account_id)
        .await
        .unwrap();
    assert_eq!(bindings, vec![binding]);
    assert_eq!(
        bindings[0].display_name.as_deref(),
        Some("Cartão •••• 4242")
    );
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_name='payment_method_bindings' ORDER BY column_name",
    )
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert!(!columns
        .iter()
        .any(|column| matches!(column.as_str(), "card_number" | "cvc" | "pan")));
}

#[tokio::test]
async fn payment_method_binding_removal_keeps_history_and_is_idempotent() {
    let fixture = setup_usage(1, 1, 0).await;
    let connection = fixture
        .repository
        .create_billing_connection(
            fixture.account_id,
            &CreateBillingConnectionRequest {
                provider: "STRIPE".into(),
                external_account_reference: "cus_remove_test".into(),
                secret_reference: "env://STRIPE_SECRET_KEY".into(),
                webhook_secret_reference: "env://STRIPE_WEBHOOK_SECRET".into(),
            },
        )
        .await
        .unwrap();
    let binding = fixture
        .repository
        .create_verified_payment_method_binding(
            fixture.account_id,
            connection.billing_connection_id,
            None,
            "pm_remove_test",
        )
        .await
        .unwrap();

    let existing = fixture
        .repository
        .find_payment_method_binding_for_removal(
            fixture.account_id,
            binding.payment_method_binding_id,
        )
        .await
        .unwrap();
    assert_eq!(existing.binding.status, "ACTIVE");
    fixture
        .repository
        .mark_payment_method_binding_detached(fixture.account_id, binding.payment_method_binding_id)
        .await
        .unwrap();
    fixture
        .repository
        .mark_payment_method_binding_detached(fixture.account_id, binding.payment_method_binding_id)
        .await
        .unwrap();

    let listed = fixture
        .repository
        .list_payment_method_bindings(fixture.account_id)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, "DETACHED");
}

#[tokio::test]
async fn stripe_detach_sends_payment_method_and_connected_account() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"pm_remove_test","object":"payment_method","customer":null}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base(
        "sk_test_remove".into(),
        Some("acct_connected_test".into()),
        server.api_base(),
    );

    connector
        .detach_payment_method("pm_remove_test")
        .await
        .unwrap();
    let request = server.finish().await;

    assert!(request.starts_with("POST /v1/payment_methods/pm_remove_test/detach HTTP/1.1"));
    assert!(request.contains("stripe-account: acct_connected_test"));
    assert!(request.contains("idempotency-key: subscription:payment-method-detach:pm_remove_test"));
}

#[tokio::test]
async fn stripe_billing_portal_session_is_hosted_and_scoped_to_connected_account() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"bps_test","url":"https://billing.stripe.com/session/test"}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base(
        "sk_test_portal".into(),
        Some("acct_connected_portal".into()),
        server.api_base(),
    );

    let url = connector
        .create_billing_portal_session("cus_test_portal", "https://tasklab.example/account")
        .await
        .unwrap();
    let request = server.finish().await.to_ascii_lowercase();

    assert_eq!(url, "https://billing.stripe.com/session/test");
    assert!(request.starts_with("post /v1/billing_portal/sessions http/1.1"));
    assert!(request.contains("stripe-account: acct_connected_portal"));
    assert!(request.contains("customer=cus_test_portal"));
    assert!(request.contains("return_url=https%3a%2f%2ftasklab.example%2faccount"));
}

#[tokio::test]
async fn stripe_subscription_cancellation_is_scheduled_at_period_end() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"sub_test_cancel","cancel_at_period_end":true}"#,
    )
    .await;
    let connector =
        StripeConnector::with_api_base("sk_test_cancel".into(), None, server.api_base());

    connector
        .schedule_subscription_cancellation("sub_test_cancel")
        .await
        .unwrap();
    let request = server.finish().await;

    assert!(request.starts_with("POST /v1/subscriptions/sub_test_cancel HTTP/1.1"));
    assert!(request.contains("cancel_at_period_end=true"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stripe_payment_intent_uses_stable_idempotency_and_domain_metadata() {
    let server =
        FakeStripeServer::responding_with(r#"{"id":"pi_test","status":"processing"}"#).await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let result = connector
        .start_collection(&CollectionCommand {
            provider_idempotency_key: "collection:11111111-1111-4111-8111-111111111111:attempt:1"
                .into(),
            payment_method: BillingPaymentMethod::Card,
            payment_method_reference: "pm_test".into(),
            customer_reference: Some("cus_test".into()),
            amount_minor: 1500,
            currency: "BRL".into(),
        })
        .await
        .unwrap();
    let request = server.finish().await;
    assert_eq!(result.state, ConnectorCollectionState::Pending);
    assert!(request
        .to_ascii_lowercase()
        .contains("idempotency-key: collection:11111111-1111-4111-8111-111111111111:attempt:1"));
    assert!(request
        .contains("metadata%5Bcollection_request_id%5D=11111111-1111-4111-8111-111111111111"));
    assert!(request.contains("payment_method=pm_test"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stripe_setup_intent_uses_tokenized_card_and_off_session_contract() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"cs_test_01","url":"https://checkout.stripe.com/c/pay/cs_test_01"}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let setup = connector
        .create_setup_session(&SetupSessionCommand {
            customer_reference: "cus_test".into(),
            client_reference_id: "plan_test".into(),
            billing_connection_id: "connection_test".into(),
            success_url: "http://localhost:5174/?session_id={CHECKOUT_SESSION_ID}".into(),
            cancel_url: "http://localhost:5174/?payment_setup=cancelled".into(),
        })
        .await
        .unwrap();
    let request = server.finish().await;
    assert_eq!(setup.provider_setup_id, "cs_test_01");
    assert_eq!(
        setup.redirect_url,
        "https://checkout.stripe.com/c/pay/cs_test_01"
    );
    assert!(request.starts_with("POST /v1/checkout/sessions "));
    assert!(request.contains("mode=setup"));
    assert!(request.contains("customer=cus_test"));
    assert!(request.contains("client_reference_id=plan_test"));
    assert!(request.contains("metadata%5Bbilling_connection_id%5D=connection_test"));
    assert!(request.contains("success_url=http%3A%2F%2Flocalhost%3A5174"));
    assert!(request.contains("cancel_url=http%3A%2F%2Flocalhost%3A5174"));
    assert!(request.contains("payment_method_types%5B%5D=card"));
    assert!(!request.contains("setup_intent_data"));
    assert!(!request.contains("return_url"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_payment_collects_and_saves_the_card_in_the_same_checkout() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"cs_test_pay","url":"https://checkout.stripe.com/c/pay/cs_test_pay"}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let session = connector
        .create_hosted_payment_session(&HostedPaymentSessionCommand {
            customer_reference: "cus_test".into(),
            client_reference_id: "checkout_test".into(),
            collection_request_id: "collection_test".into(),
            amount_minor: 2_500,
            currency: "BRL".into(),
            success_url: "https://client.example/done?session_id={CHECKOUT_SESSION_ID}".into(),
            cancel_url: "https://client.example/cancel".into(),
            expires_at: 1_900_000_000,
            provider_idempotency_key: "checkout:checkout_test:hosted-session:v2".into(),
            allow_payment_method_save: true,
            is_subscription: false,
            customer_plan_id: "plan_test".into(),
        })
        .await
        .unwrap();
    let request = server.finish().await;
    assert_eq!(session.provider_session_id, "cs_test_pay");
    assert!(request.contains("mode=payment"));
    assert!(request.contains("line_items%5B0%5D%5Bprice_data%5D%5Bunit_amount%5D=2500"));
    assert!(request.contains("saved_payment_method_options%5Bpayment_method_save%5D=enabled"));
    assert!(request.contains("metadata%5Bcollection_request_id%5D=collection_test"));
    assert!(
        request.contains("session_id%3D{CHECKOUT_SESSION_ID}"),
        "Stripe form must keep its checkout macro literal: {request}"
    );
    assert!(request.contains("idempotency-key: checkout:checkout_test:hosted-session:v2"));
    assert!(!request.contains("mode=setup"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_subscription_uses_monthly_stripe_billing_and_plan_metadata() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"cs_test_subscription","url":"https://checkout.stripe.com/c/pay/cs_test_subscription"}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    connector
        .create_hosted_payment_session(&HostedPaymentSessionCommand {
            customer_reference: "cus_test".into(),
            client_reference_id: "checkout_test".into(),
            collection_request_id: "collection_test".into(),
            amount_minor: 100,
            currency: "BRL".into(),
            success_url: "https://client.example/done?session_id={CHECKOUT_SESSION_ID}".into(),
            cancel_url: "https://client.example/cancel".into(),
            expires_at: 1_900_000_000,
            provider_idempotency_key: "checkout:checkout_test:hosted-session:v2".into(),
            allow_payment_method_save: false,
            is_subscription: true,
            customer_plan_id: "plan_test".into(),
        })
        .await
        .unwrap();
    let request = server.finish().await;
    assert!(request.contains("mode=subscription"));
    assert!(
        request.contains("line_items%5B0%5D%5Bprice_data%5D%5Brecurring%5D%5Binterval%5D=month"),
        "Stripe Checkout should receive recurring month terms: {request}"
    );
    assert!(request.contains("subscription_data%5Bmetadata%5D%5Bcustomer_plan_id%5D=plan_test"));
    assert!(!request.contains("saved_payment_method_options"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_payment_retrieval_requires_paid_session_and_saved_method() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"cs_test_pay","customer":"cus_test","client_reference_id":"checkout_test","mode":"payment","payment_status":"paid","amount_total":2500,"currency":"brl","payment_intent":{"id":"pi_test_pay","status":"succeeded","payment_method":{"id":"pm_saved_test","allow_redisplay":"always","card":{"brand":"visa","last4":"4242","exp_month":12,"exp_year":2030}}}}"#,
    ).await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let receipt = connector
        .retrieve_hosted_payment("cs_test_pay", "cus_test", "checkout_test", "payment")
        .await
        .unwrap();
    let request = server.finish().await;
    assert!(request.starts_with(
        "GET /v1/checkout/sessions/cs_test_pay?expand%5B%5D=payment_intent.payment_method&expand%5B%5D=subscription.latest_invoice.payment_intent.payment_method HTTP/1.1"
    ));
    assert_eq!(receipt.payment_intent_id, "pi_test_pay");
    assert_eq!(receipt.payment_method_id, "pm_saved_test");
    assert!(receipt.saved_for_future);
    let card = receipt.card.expect("Stripe card summary");
    assert_eq!(
        (card.brand.as_str(), card.last_four.as_str()),
        ("visa", "4242")
    );
    assert_eq!((card.exp_month, card.exp_year), (12, 2030));
    assert_eq!(
        (receipt.amount_minor, receipt.currency.as_str()),
        (2_500, "BRL")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stripe_direct_card_setup_confirms_card_without_echoing_card_secrets() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"seti_card_test","status":"succeeded","customer":"cus_card_test","payment_method":"pm_card_test","usage":"off_session"}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let result = connector
        .create_card_setup_intent(
            "cus_card_test",
            "TaskLab Test",
            "4242424242424242",
            12,
            2035,
            "123",
            "card-setup-test",
        )
        .await
        .unwrap();
    let request = server.finish().await;
    assert_eq!(result.payment_method_id, "pm_card_test");
    assert!(request.starts_with("POST /v1/setup_intents "));
    assert!(request.contains("usage=off_session"));
    assert!(request.contains("payment_method_data%5Bcard%5D%5Bnumber%5D=4242424242424242"));
    assert!(request.contains("payment_method_data%5Bcard%5D%5Bcvc%5D=123"));
    assert!(request.contains("idempotency-key: card-setup-test"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stripe_declined_direct_card_setup_returns_safe_provider_error() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"seti_card_failed","status":"requires_payment_method","client_secret":"do_not_return","last_setup_error":{"code":"card_declined","message":"card declined"}}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let error = connector
        .create_card_setup_intent(
            "cus_card_test",
            "TaskLab Test",
            "4242424242424242",
            12,
            2035,
            "123",
            "card-setup-failure-test",
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "card_declined");
    assert!(!error.message.contains("do_not_return"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stripe_setup_verification_reads_provider_state_and_requires_off_session_use() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"cs_test_verified","mode":"setup","status":"complete","customer":"cus_verified","client_reference_id":"plan_verified","metadata":{"billing_connection_id":"connection_verified"},"setup_intent":{"id":"seti_verified","status":"succeeded","customer":"cus_verified","payment_method":"pm_verified","usage":"off_session"}}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let payment_method = connector
        .retrieve_checkout_setup_intent(
            "cs_test_verified",
            "cus_verified",
            "plan_verified",
            "connection_verified",
        )
        .await
        .unwrap();
    let request = server.finish().await;
    assert!(request.starts_with(
        "GET /v1/checkout/sessions/cs_test_verified?expand%5B%5D=setup_intent.payment_method "
    ));
    assert_eq!(payment_method.customer_id, "cus_verified");
    assert_eq!(payment_method.payment_method_id, "pm_verified");

    let server = FakeStripeServer::responding_with(
        r#"{"id":"cs_test_verified","mode":"setup","status":"complete","customer":"cus_verified","client_reference_id":"plan_verified","metadata":{"billing_connection_id":"connection_verified"},"setup_intent":{"id":"seti_verified","status":"succeeded","customer":"cus_verified","payment_method":"pm_verified","usage":"on_session"}}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    assert!(connector
        .retrieve_checkout_setup_intent(
            "cs_test_verified",
            "cus_verified",
            "plan_verified",
            "connection_verified"
        )
        .await
        .is_err());
    let _ = server.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sandbox_payment_method_is_selected_inside_billing_and_bound_to_customer() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"seti_sandbox","status":"succeeded","customer":"cus_sandbox","payment_method":"pm_card_visa"}"#,
    ).await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let setup = connector
        .prepare_test_payment_method("cus_sandbox", "checkout:demo:payment-method:v1", false)
        .await
        .unwrap();
    let request = server.finish().await;
    assert_eq!(setup.payment_method_id, "pm_card_visa");
    assert_eq!(setup.customer_id, "cus_sandbox");
    assert!(request.contains("payment_method=pm_card_visa"));
    assert!(request.contains("confirm=true"));
    assert!(request.contains("usage=off_session"));
    assert!(request.contains("idempotency-key: checkout:demo:payment-method:v1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn declined_sandbox_scenario_selects_a_failure_method_without_client_input() {
    let server = FakeStripeServer::responding_with(
        r#"{"id":"seti_declined","status":"succeeded","customer":"cus_sandbox","payment_method":"pm_card_chargeCustomerFail"}"#,
    ).await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let setup = connector
        .prepare_test_payment_method("cus_sandbox", "checkout:demo:declined:v1", true)
        .await
        .unwrap();
    let request = server.finish().await;
    assert_eq!(setup.payment_method_id, "pm_card_chargeCustomerFail");
    assert!(request.contains("payment_method=pm_card_chargeCustomerFail"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_refund_is_observed_once_without_automatic_credit_effect() {
    let fixture = setup_usage(1, 1, 25).await;
    let connection = billing::create_billing_connection(
        &fixture.repository,
        fixture.account_id,
        &CreateBillingConnectionRequest {
            provider: "STRIPE".into(),
            external_account_reference: "cus_refund".into(),
            secret_reference: "env://STRIPE_SECRET_KEY".into(),
            webhook_secret_reference: "env://STRIPE_WEBHOOK_SECRET".into(),
        },
    )
    .await
    .unwrap();
    let first = fixture
        .repository
        .record_external_refund(
            connection.billing_connection_id,
            "evt_refund",
            "pi_refund",
            500,
            "BRL",
            &"e".repeat(64),
        )
        .await
        .unwrap();
    let duplicate = fixture
        .repository
        .record_external_refund(
            connection.billing_connection_id,
            "evt_refund",
            "pi_refund",
            500,
            "BRL",
            &"e".repeat(64),
        )
        .await
        .unwrap();
    assert!(first);
    assert!(!duplicate);
    let balance: i64 = sqlx::query_scalar(
        "SELECT cw.balance_credit_units FROM customer_wallets cw \
         JOIN wallets w USING(wallet_id) WHERE w.customer_id=$1 AND w.wallet_type='CUSTOMER'",
    )
    .bind(fixture.account_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(balance, 25);
}
