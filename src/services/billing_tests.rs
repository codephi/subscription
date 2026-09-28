use std::sync::{Arc, Mutex};

use super::*;
use crate::repositories::billing_connector::{
    BillingCapabilities, BillingPaymentMethod, ConnectorCollectionState, ConnectorFuture,
};

struct FakeBillingConnector {
    capabilities: BillingCapabilities,
    calls: Arc<Mutex<Vec<CollectionCommand>>>,
}

impl BillingConnector for FakeBillingConnector {
    fn capabilities(&self) -> BillingCapabilities {
        self.capabilities.clone()
    }

    fn start_collection<'a>(&'a self, command: &'a CollectionCommand) -> ConnectorFuture<'a> {
        let calls = Arc::clone(&self.calls);
        let command = command.clone();
        Box::pin(async move {
            calls.lock().expect("fake calls lock").push(command);
            Ok(ConnectorCollectionResult {
                provider_payment_id: Some("fake-payment-1".to_string()),
                state: ConnectorCollectionState::Pending,
                failure_code: None,
                next_action_url: None,
            })
        })
    }
}

fn command(amount_minor: i64) -> CollectionCommand {
    CollectionCommand {
        provider_idempotency_key: "collection-request-1:1".to_string(),
        payment_method: BillingPaymentMethod::Card,
        payment_method_reference: "pm_fake".to_string(),
        customer_reference: Some("cus_fake".to_string()),
        amount_minor,
        currency: "BRL".to_string(),
    }
}

#[test]
fn payment_setup_return_urls_require_same_secure_origin_and_checkout_session_marker() {
    assert!(validate_setup_return_urls(
        "https://tasklab.example/?session_id={CHECKOUT_SESSION_ID}",
        "https://tasklab.example/?payment_setup=cancelled",
    )
    .is_ok());
    assert!(validate_setup_return_urls(
        "http://localhost:5174/?session_id={CHECKOUT_SESSION_ID}",
        "http://localhost:5174/?payment_setup=cancelled",
    )
    .is_ok());
    assert!(validate_setup_return_urls(
        "https://evil.example/?session_id={CHECKOUT_SESSION_ID}",
        "https://tasklab.example/?payment_setup=cancelled",
    )
    .is_err());
    assert!(validate_setup_return_urls(
        "http://tasklab.example/?session_id={CHECKOUT_SESSION_ID}",
        "http://tasklab.example/?payment_setup=cancelled",
    )
    .is_err());
}

fn billing_capabilities(methods: Vec<BillingPaymentMethod>) -> BillingCapabilities {
    BillingCapabilities {
        payment_methods: methods,
        supports_setup_session: true,
        supports_vault: true,
        supports_off_session_charge: true,
        supports_webhook: true,
    }
}

#[tokio::test]
async fn fake_billing_connector_receives_stable_collection_command() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let connector = FakeBillingConnector {
        capabilities: billing_capabilities(vec![BillingPaymentMethod::Card]),
        calls: Arc::clone(&calls),
    };
    let result = start_collection(&connector, &command(1_500))
        .await
        .expect("fake collection");
    assert_eq!(result.state, ConnectorCollectionState::Pending);
    assert_eq!(calls.lock().expect("fake calls lock").len(), 1);
}

#[tokio::test]
async fn unsupported_capability_and_invalid_amount_skip_external_call() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let connector = FakeBillingConnector {
        capabilities: billing_capabilities(Vec::new()),
        calls: Arc::clone(&calls),
    };
    let capability = start_collection(&connector, &command(1_500))
        .await
        .expect_err("unsupported card");
    assert_eq!(capability.code(), "billing_capability_not_supported");
    let amount = start_collection(&connector, &command(0))
        .await
        .expect_err("zero amount");
    assert_eq!(amount.code(), "invalid_collection_amount");
    assert!(calls.lock().expect("fake calls lock").is_empty());
}

#[test]
fn payment_binding_ingestion_requires_a_checkout_session_reference() {
    let error = validate_checkout_session_reference("pm_client_supplied")
        .expect_err("pm ID is not a checkout receipt");
    assert_eq!(error.code(), "invalid_checkout_session_reference");
    assert!(validate_checkout_session_reference("cs_testProvider123").is_ok());
}
