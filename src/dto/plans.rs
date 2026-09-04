use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::units::CreditUnits;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubscriptionModel {
    CreditStrict,
    CreditFlexible,
    EntitlementOnly,
}

impl SubscriptionModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreditStrict => "CREDIT_STRICT",
            Self::CreditFlexible => "CREDIT_FLEXIBLE",
            Self::EntitlementOnly => "ENTITLEMENT_ONLY",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommercialModel {
    Free,
    Paid,
}

impl CommercialModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "FREE",
            Self::Paid => "PAID",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanRecurrence {
    None,
    Weekly,
    Monthly,
    Quarterly,
    Annually,
}

impl PlanRecurrence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Weekly => "WEEKLY",
            Self::Monthly => "MONTHLY",
            Self::Quarterly => "QUARTERLY",
            Self::Annually => "ANNUALLY",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdmissionPolicy {
    Open,
    ApprovalRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanTransitionKind {
    Upgrade,
    Downgrade,
}

impl PlanTransitionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upgrade => "UPGRADE",
            Self::Downgrade => "DOWNGRADE",
        }
    }
}

impl AdmissionPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::ApprovalRequired => "APPROVAL_REQUIRED",
        }
    }
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateSubscriptionRequest {
    pub name: String,
    pub subscription_model: SubscriptionModel,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SubscriptionResponse {
    pub subscription_id: Uuid,
    pub name: String,
    pub subscription_model: SubscriptionModel,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateSubscriptionPlanRequest {
    pub name: String,
    pub commercial_model: CommercialModel,
    pub price_amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub recurrence: PlanRecurrence,
    pub admission_policy: AdmissionPolicy,
    #[serde(default)]
    pub accepted_payment_methods: Vec<String>,
    pub granted_credit_units: CreditUnits,
    pub product_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SubscriptionPlanResponse {
    pub plan_version_id: Uuid,
    pub subscription_id: Uuid,
    pub name: String,
    pub commercial_model: CommercialModel,
    pub price_amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub recurrence: PlanRecurrence,
    pub admission_policy: AdmissionPolicy,
    pub accepted_payment_methods: Vec<String>,
    pub granted_credit_units: CreditUnits,
    pub product_ids: Vec<Uuid>,
    pub published_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateOnDemandPlanRequest {
    pub name: String,
    pub price_amount_minor: i64,
    pub currency: String,
    pub credit_units: CreditUnits,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OnDemandPlanResponse {
    pub on_demand_plan_id: Uuid,
    pub subscription_id: Uuid,
    pub name: String,
    pub price_amount_minor: i64,
    pub currency: String,
    pub credit_units: CreditUnits,
    pub published_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateCustomerPlanRequest {
    pub plan_version_id: Uuid,
    pub transaction_id: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomerPlanCycleResponse {
    pub customer_plan_cycle_id: Uuid,
    pub cycle_ordinal: i64,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub granted_credit_units: CreditUnits,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomerPlanResponse {
    pub customer_plan_id: Uuid,
    pub customer_id: Uuid,
    pub plan_version_id: Uuid,
    pub commercial_status: String,
    pub activation_status: String,
    pub renewal_status: String,
    pub anchor_at: DateTime<Utc>,
    pub cancel_at_period_end: bool,
    pub ended_at: Option<DateTime<Utc>>,
    pub end_reason: Option<String>,
    pub version: i64,
    pub current_cycle: Option<CustomerPlanCycleResponse>,
    pub entitled_product_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct RevokePlanRequest {
    pub reason: String,
    pub actor_reference: String,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct RevokeCustomerPlanRequest {
    pub reason: String,
    pub actor_reference: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreatePlanTransitionRequest {
    pub new_plan_version_id: Uuid,
    pub transition_kind: PlanTransitionKind,
    pub transaction_id: String,
    pub actor_reference: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PlanTransitionResponse {
    pub plan_transition_id: Uuid,
    pub transition_kind: PlanTransitionKind,
    pub previous_plan_version_id: Uuid,
    pub new_plan_version_id: Uuid,
    pub reclassified_credit_units: CreditUnits,
    pub customer_plan: CustomerPlanResponse,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct RunSubscriptionCyclesRequest {
    pub as_of: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RunSubscriptionCyclesResponse {
    pub processed_customer_plans: i64,
    pub created_cycles: i64,
    pub canceled_customer_plans: i64,
}
