use serde_json::json;
use subscription::{
    dto::{
        events::WorkspaceEventEnvelope,
        plans::{
            AdmissionPolicy, CommercialModel, CreateCustomerPlanRequest,
            CreateSubscriptionPlanRequest, CreateSubscriptionRequest, PlanRecurrence,
            RevokePlanRequest, SubscriptionModel,
        },
        units::CreditUnits,
    },
    repositories::database::DatabaseRepository,
    services::workspace_events::process_workspace_event,
};
use uuid::Uuid;

pub fn subscription_request() -> CreateSubscriptionRequest {
    CreateSubscriptionRequest {
        name: format!("Subscription {}", Uuid::new_v4()),
        subscription_model: SubscriptionModel::CreditStrict,
    }
}

pub fn plan_request(
    product_id: Uuid,
    commercial_model: CommercialModel,
    recurrence: PlanRecurrence,
    credits: i64,
) -> CreateSubscriptionPlanRequest {
    CreateSubscriptionPlanRequest {
        admission_policy_version_id: None,
        name: format!("Plan {}", Uuid::new_v4()),
        commercial_model,
        price_amount_minor: None,
        currency: None,
        recurrence,
        admission_policy: AdmissionPolicy::Open,
        accepted_payment_methods: Vec::new(),
        granted_credit_units: CreditUnits::new(credits),
        product_ids: vec![product_id],
    }
}

pub fn paid_plan_request(product_id: Uuid) -> CreateSubscriptionPlanRequest {
    CreateSubscriptionPlanRequest {
        admission_policy_version_id: None,
        name: format!("Paid plan {}", Uuid::new_v4()),
        commercial_model: CommercialModel::Paid,
        price_amount_minor: Some(1_000),
        currency: Some("BRL".to_string()),
        recurrence: PlanRecurrence::Monthly,
        admission_policy: AdmissionPolicy::Open,
        accepted_payment_methods: vec!["CARD".to_string()],
        granted_credit_units: CreditUnits::new(100),
        product_ids: vec![product_id],
    }
}

pub fn customer_plan_request(
    plan_version_id: Uuid,
    transaction_id: &str,
) -> CreateCustomerPlanRequest {
    CreateCustomerPlanRequest {
        plan_version_id,
        transaction_id: transaction_id.to_string(),
    }
}

pub fn revoke_plan_request() -> RevokePlanRequest {
    RevokePlanRequest {
        reason: "offer withdrawn".to_string(),
        actor_reference: "operator:test".to_string(),
    }
}

pub async fn apply_workspace_event(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    kind: &str,
    sequence: i64,
) {
    let event: WorkspaceEventEnvelope = serde_json::from_value(json!({
        "event_id":Uuid::new_v4(),"event_type":kind,"schema_version":1,"aggregate_id":workspace_id,
        "sequence":sequence,"occurred_at":"2026-09-04T00:00:00Z","workspace_id":workspace_id,
        "correlation_id":Uuid::new_v4(),"causation_id":null,"payload":{"workspace_id":workspace_id}
    }))
    .expect("workspace event");
    process_workspace_event(repository, event)
        .await
        .expect("apply workspace event");
}
