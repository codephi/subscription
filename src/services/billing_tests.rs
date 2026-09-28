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
fn payment_setup_return_urls_require_same_secure_origin() {
    assert!(validate_setup_return_urls(
        "https://tasklab.example/?payment_setup=complete",
        "https://tasklab.example/?payment_setup=cancelled",
    )
    .is_ok());
    assert!(validate_setup_return_urls(
        "http://localhost:5174/?payment_setup=complete",
        "http://localhost:5174/?payment_setup=cancelled",
    )
    .is_ok());
    assert!(validate_setup_return_urls(
        "https://evil.example/?payment_setup=complete",
        "https://tasklab.example/?payment_setup=cancelled",
    )
    .is_err());
    assert!(validate_setup_return_urls(
        "http://tasklab.example/?payment_setup=complete",
        "http://tasklab.example/?payment_setup=cancelled",
    )
    .is_err());
}

#[test]
fn payment_setup_return_reference_is_added_by_subscription() {
    let setup_id = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    assert_eq!(
        setup_success_return_url("https://tasklab.example/return?payment_setup=complete", setup_id)
            .unwrap(),
        "https://tasklab.example/return?payment_setup=complete&payment_method_setup_id=11111111-1111-4111-8111-111111111111"
    );
    assert!(setup_success_return_url(
        "https://tasklab.example/return?payment_method_setup_id=caller-value",
        setup_id
    )
    .is_err());
}

#[test]
fn customer_payment_method_binding_hides_provider_integration_details() {
    let public: crate::dto::billing::CustomerPaymentMethodBindingResponse =
        crate::dto::billing::PaymentMethodBindingResponse {
            payment_method_binding_id: uuid::Uuid::new_v4(),
            billing_connection_id: uuid::Uuid::new_v4(),
            workspace_id: uuid::Uuid::new_v4(),
            customer_plan_id: Some(uuid::Uuid::new_v4()),
            payment_method: "CARD".to_string(),
            status: "ACTIVE".to_string(),
            created_at: chrono::Utc::now(),
        }
        .into();
    let body = serde_json::to_value(public).unwrap();
    assert!(body.get("billing_connection_id").is_none());
    assert!(body.get("provider_payment_method_reference").is_none());
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
