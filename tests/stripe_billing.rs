mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use std::sync::{Arc, Mutex};

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subscription::{
    dto::{
        billing::{
            CreateBillingConnectionRequest, CreateInitialCollectionRequest,
            CreatePaymentMethodBindingRequest,
        },
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
            ConnectorCollectionState, SetupSessionCommand,
        },
        stripe::StripeConnector,
    },
    services::{billing, plans, stripe_webhooks},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

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
            fixture.workspace_id,
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
                fixture.workspace_id,
                integration.billing_connection_id,
                "stripe_api",
                &stored,
            )
            .unwrap(),
        "sk_test_managed_secret"
    );

    let configured = repository
        .update_stripe_integration(
            fixture.workspace_id,
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
        .list_integrations(fixture.workspace_id)
        .await
        .unwrap();
    let public_summary = serde_json::to_string(&summaries).unwrap();
    assert!(!public_summary.contains("whsec_managed_test"));
    assert!(!public_summary.contains("sk_test_managed_secret"));
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
                body.len(), body
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
        fixture.workspace_id,
        "stripe-plan-key",
        CreateCustomerPlanRequest {
            plan_version_id: plan.plan_version_id,
            transaction_id: "stripe-plan-transaction".into(),
        },
    )
    .await
    .unwrap();
    let secret_variable = format!("STRIPE_WEBHOOK_SECRET_{}", fixture.workspace_id.simple());
    std::env::set_var(&secret_variable, "whsec_integration");
    let connection = billing::create_billing_connection(
        &fixture.repository,
        fixture.workspace_id,
        &CreateBillingConnectionRequest {
            provider: "STRIPE".into(),
            external_account_reference: "cus_webhook".into(),
            secret_reference: "env://STRIPE_SECRET_KEY_UNUSED".into(),
            webhook_secret_reference: format!("env://{secret_variable}"),
        },
    )
    .await
    .unwrap();
    let binding = billing::create_payment_method_binding(
        &fixture.repository,
        fixture.workspace_id,
        &CreatePaymentMethodBindingRequest {
            billing_connection_id: connection.billing_connection_id,
            customer_plan_id: Some(customer_plan.customer_plan_id),
            provider_payment_method_reference: "pm_webhook".into(),
        },
    )
    .await
    .unwrap();
    let collection = billing::create_initial_collection(
        &fixture.repository,
        fixture.workspace_id,
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
    .bind(fixture.workspace_id)
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
        fixture.workspace_id,
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
        fixture.workspace_id,
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
    let binding = billing::create_payment_method_binding(
        &fixture.repository,
        fixture.workspace_id,
        &CreatePaymentMethodBindingRequest {
            billing_connection_id: connection.billing_connection_id,
            customer_plan_id: None,
            provider_payment_method_reference: "pm_tokenized_test".into(),
        },
    )
    .await
    .unwrap();
    let bindings = billing::list_payment_method_bindings(&fixture.repository, fixture.workspace_id)
        .await
        .unwrap();
    assert_eq!(bindings, vec![binding]);
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
        r#"{"id":"seti_test","client_secret":"seti_test_secret"}"#,
    )
    .await;
    let connector = StripeConnector::with_api_base("sk_test".into(), None, server.api_base());
    let setup = connector
        .create_setup_session(&SetupSessionCommand {
            customer_reference: "cus_test".into(),
            return_url: "https://example.test/billing/return".into(),
        })
        .await
        .unwrap();
    let request = server.finish().await;
    assert_eq!(setup.provider_setup_id, "seti_test");
    assert_eq!(setup.client_secret, "seti_test_secret");
    assert!(request.starts_with("POST /v1/setup_intents "));
    assert!(request.contains("customer=cus_test"));
    assert!(request.contains("payment_method_types%5B%5D=card"));
    assert!(request.contains("usage=off_session"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_refund_is_observed_once_without_automatic_credit_effect() {
    let fixture = setup_usage(1, 1, 25).await;
    let connection = billing::create_billing_connection(
        &fixture.repository,
        fixture.workspace_id,
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
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(balance, 25);
}
