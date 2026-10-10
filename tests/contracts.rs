use serde_json::Value;

const ACCOUNTS_EVENT_SCHEMA: &str =
    include_str!("../docs/contracts/accounts-account-event-v1.schema.json");
const DOMAIN_EVENT_SCHEMA: &str =
    include_str!("../docs/contracts/subscription-domain-event-v1.schema.json");

#[test]
fn contract_documents_cover_phase_zero_decisions() {
    let accounts_schema: Value =
        serde_json::from_str(ACCOUNTS_EVENT_SCHEMA).expect("accounts schema must be valid JSON");
    let domain_schema: Value =
        serde_json::from_str(DOMAIN_EVENT_SCHEMA).expect("domain schema must be valid JSON");

    assert_required_fields(
        &accounts_schema,
        &[
            "event_id",
            "event_type",
            "sequence",
            "account_id",
            "correlation_id",
        ],
    );
    assert_required_fields(
        &domain_schema,
        &[
            "event_id",
            "event_type",
            "sequence",
            "account_id",
            "correlation_id",
        ],
    );

    let accounts_boundary = include_str!("../docs/adr/0003-accounts-boundary.md");
    let event_delivery = include_str!("../docs/adr/0004-signed-http-events.md");
    assert!(accounts_boundary.contains("autenticação geral foi adiada"));
    assert!(event_delivery.contains("HMAC-SHA256"));
    assert!(event_delivery.contains("quarentena"));
    assert!(!ACCOUNTS_EVENT_SCHEMA.contains("workspace"));
    assert!(!DOMAIN_EVENT_SCHEMA.contains("workspace"));
    assert!(ACCOUNTS_EVENT_SCHEMA.contains("account.created"));
    assert!(ACCOUNTS_EVENT_SCHEMA.contains("account.activated"));
    assert!(DOMAIN_EVENT_SCHEMA.contains("account_provisioning.started"));
    assert!(DOMAIN_EVENT_SCHEMA.contains("account_provisioning.completed"));
    assert!(DOMAIN_EVENT_SCHEMA.contains("account_provisioning.failed"));
    assert!(DOMAIN_EVENT_SCHEMA.contains("credit.granted"));
    assert!(DOMAIN_EVENT_SCHEMA.contains("customer_plan.activated"));
    assert!(DOMAIN_EVENT_SCHEMA.contains("customer_plan.plan_changed"));
    assert!(DOMAIN_EVENT_SCHEMA.contains("credit.expired"));
}

#[test]
fn account_event_contract_rejects_workspace_field_names() {
    let schema: Value = serde_json::from_str(ACCOUNTS_EVENT_SCHEMA).expect("valid account schema");
    let properties = schema["properties"].as_object().expect("properties object");
    assert!(properties.contains_key("account_id"));
    assert!(!properties.contains_key("workspace_id"));
    let payload = properties["payload"]["properties"]
        .as_object()
        .expect("payload properties object");
    assert!(payload.contains_key("account_id"));
    assert!(!payload.contains_key("workspace_id"));
}

fn assert_required_fields(schema: &Value, expected_fields: &[&str]) {
    let required = schema["required"]
        .as_array()
        .expect("schema.required must be an array");
    for field in expected_fields {
        assert!(
            required.iter().any(|entry| entry == field),
            "schema must require field {field}"
        );
    }
}
