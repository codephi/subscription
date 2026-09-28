use std::sync::{Arc, Mutex};

use super::*;
use crate::repositories::billing_connector::{
    BillingCapabilities, BillingPaymentMethod, ConnectorCollectionState, ConnectorFuture,
};
use chrono::Datelike;

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
fn direct_card_setup_validates_shape_and_never_echoes_card_values() {
    let valid = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        customer_plan_id: uuid::Uuid::new_v4(),
        cardholder_name: "TaskLab Test".to_string(),
        card_name: None,
        card_number: "4242424242424242".to_string(),
        exp_month: 12,
        exp_year: (chrono::Utc::now().year() + 2) as u16,
        cvc: "123".to_string(),
        save_for_future: true,
    };
    assert!(validate_card_entry(&valid).is_ok());

    let invalid = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        card_number: "4111".to_string(),
        ..valid
    };
    let error = validate_card_entry(&invalid).unwrap_err();
    assert_eq!(error.code(), "invalid_card_number");
    assert!(!error.to_string().contains("4111"));
}

#[test]
fn direct_card_setup_rejects_invalid_name_cvc_and_expiry_shape() {
    let valid = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        customer_plan_id: uuid::Uuid::new_v4(),
        cardholder_name: "TaskLab Test".to_string(),
        card_name: None,
        card_number: "4242424242424242".to_string(),
        exp_month: 12,
        exp_year: (chrono::Utc::now().year() + 2) as u16,
        cvc: "123".to_string(),
        save_for_future: false,
    };
    let invalid_name = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        cardholder_name: "  ".to_string(),
        ..valid.clone()
    };
    assert_eq!(
        validate_card_entry(&invalid_name).unwrap_err().code(),
        "invalid_cardholder_name"
    );
    let invalid_cvc = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        cvc: "12x".to_string(),
        ..valid.clone()
    };
    let error = validate_card_entry(&invalid_cvc).unwrap_err();
    assert_eq!(error.code(), "invalid_card_security_code");
    assert!(!error.to_string().contains("12x"));
    let invalid_month = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        exp_month: 13,
        ..valid
    };
    assert_eq!(
        validate_card_entry(&invalid_month).unwrap_err().code(),
        "invalid_card_expiry_month"
    );
}

#[test]
fn saved_card_name_always_includes_last_four_digits() {
    let request = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        customer_plan_id: uuid::Uuid::new_v4(),
        cardholder_name: "TaskLab Test".to_string(),
        card_name: None,
        card_number: "4242 4242 4242 4242".to_string(),
        exp_month: 12,
        exp_year: (chrono::Utc::now().year() + 2) as u16,
        cvc: "123".to_string(),
        save_for_future: true,
    };
    assert_eq!(card_display_name(&request), "Cartão •••• 4242");
    let named = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        card_name: Some("Cartão de trabalho".to_string()),
        ..request.clone()
    };
    assert_eq!(card_display_name(&named), "Cartão de trabalho •••• 4242");
    assert!(validate_card_entry(&named).is_ok());

    let invalid_name = crate::dto::billing::CreatePaymentMethodFromCardRequest {
        card_name: Some("x".repeat(51)),
        ..named
    };
    assert_eq!(
        validate_card_entry(&invalid_name).unwrap_err().code(),
        "invalid_card_display_name"
    );
    assert!(!card_display_name(&invalid_name).contains(&invalid_name.card_number));
}

#[test]
fn direct_card_setup_only_accepts_test_credentials() {
    assert!(validate_raw_card_secret("sk_test_example").is_ok());
    let error = validate_raw_card_secret("sk_live_example").unwrap_err();
    assert_eq!(error.code(), "raw_card_setup_sandbox_only");
    assert!(!error.to_string().contains("sk_live_example"));
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
            display_name: Some("Cartão •••• 4242".to_string()),
            status: "ACTIVE".to_string(),
            created_at: chrono::Utc::now(),
        }
        .into();
    let body = serde_json::to_value(public).unwrap();
    assert!(body.get("billing_connection_id").is_none());
    assert!(body.get("provider_payment_method_reference").is_none());
    assert_eq!(body["display_name"], "Cartão •••• 4242");
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
