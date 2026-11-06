use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::{
        plans::{
            AdmissionPolicy, CommercialModel, CustomerPlanCycleResponse, CustomerPlanResponse,
            OnDemandPlanResponse, PlanRecurrence, SubscriptionModel, SubscriptionPlanResponse,
            SubscriptionResponse,
        },
        units::CreditUnits,
    },
    error::{ApiError, ApiResult},
};

#[derive(Clone)]
pub(crate) struct PlanRecord {
    pub(crate) response: SubscriptionPlanResponse,
}

pub(super) fn subscription_from_row(
    row: &sqlx::postgres::PgRow,
) -> ApiResult<SubscriptionResponse> {
    Ok(SubscriptionResponse {
        subscription_id: row.get("subscription_id"),
        name: row.get("name"),
        subscription_model: parse_subscription_model(row.get("subscription_model"))?,
        created_at: row.get("created_at"),
    })
}

pub(super) fn plan_from_row(
    row: &sqlx::postgres::PgRow,
    product_ids: Vec<Uuid>,
) -> ApiResult<SubscriptionPlanResponse> {
    Ok(SubscriptionPlanResponse {
        admission_policy_version_id: row.get("admission_policy_version_id"),
        plan_version_id: row.get("plan_version_id"),
        subscription_id: row.get("subscription_id"),
        name: row.get("name"),
        commercial_model: parse_commercial_model(row.get("commercial_model"))?,
        price_amount_minor: row.get("price_amount_minor"),
        currency: row.get("currency"),
        recurrence: parse_recurrence(row.get("recurrence"))?,
        admission_policy: parse_admission(row.get("admission_policy"))?,
        accepted_payment_methods: row.get("accepted_payment_methods"),
        granted_credit_units: CreditUnits::new(row.get("granted_credit_units")),
        product_ids,
        published_at: row.get("published_at"),
        revoked_at: row.get("revoked_at"),
    })
}

pub(super) fn on_demand_from_row(row: &sqlx::postgres::PgRow) -> OnDemandPlanResponse {
    OnDemandPlanResponse {
        on_demand_plan_id: row.get("on_demand_plan_id"),
        subscription_id: row.get("subscription_id"),
        name: row.get("name"),
        price_amount_minor: row.get("price_amount_minor"),
        currency: row.get("currency"),
        credit_units: CreditUnits::new(row.get("credit_units")),
        published_at: row.get("published_at"),
        revoked_at: row.get("revoked_at"),
    }
}

pub(super) fn cycle_from_row(row: &sqlx::postgres::PgRow) -> CustomerPlanCycleResponse {
    CustomerPlanCycleResponse {
        customer_plan_cycle_id: row.get("customer_plan_cycle_id"),
        cycle_ordinal: row.get("cycle_ordinal"),
        current_period_start: row.get("current_period_start"),
        current_period_end: row.get("current_period_end"),
        granted_credit_units: CreditUnits::new(row.get("granted_credit_units")),
        status: row.get("status"),
    }
}

pub(super) fn customer_plan_from_row(
    row: &sqlx::postgres::PgRow,
    cycle: Option<CustomerPlanCycleResponse>,
    product_ids: Vec<Uuid>,
) -> CustomerPlanResponse {
    CustomerPlanResponse {
        customer_plan_id: row.get("customer_plan_id"),
        customer_id: row.get("customer_id"),
        plan_version_id: row.get("plan_version_id"),
        commercial_status: row.get("commercial_status"),
        activation_status: row.get("activation_status"),
        renewal_status: row.get("renewal_status"),
        anchor_at: row.get("anchor_at"),
        cancel_at_period_end: row.get("cancel_at_period_end"),
        ended_at: row.get("ended_at"),
        end_reason: row.get("end_reason"),
        version: row.get("version"),
        current_cycle: cycle,
        entitled_product_ids: product_ids,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub(super) fn parse_subscription_model(value: &str) -> ApiResult<SubscriptionModel> {
    match value {
        "CREDIT_STRICT" => Ok(SubscriptionModel::CreditStrict),
        "CREDIT_FLEXIBLE" => Ok(SubscriptionModel::CreditFlexible),
        "ENTITLEMENT_ONLY" => Ok(SubscriptionModel::EntitlementOnly),
        _ => Err(unknown("subscription model", value)),
    }
}

pub(super) fn parse_commercial_model(value: &str) -> ApiResult<CommercialModel> {
    match value {
        "FREE" => Ok(CommercialModel::Free),
        "PAID" => Ok(CommercialModel::Paid),
        _ => Err(unknown("commercial model", value)),
    }
}

pub(super) fn parse_recurrence(value: &str) -> ApiResult<PlanRecurrence> {
    match value {
        "NONE" => Ok(PlanRecurrence::None),
        "WEEKLY" => Ok(PlanRecurrence::Weekly),
        "MONTHLY" => Ok(PlanRecurrence::Monthly),
        "QUARTERLY" => Ok(PlanRecurrence::Quarterly),
        "ANNUALLY" => Ok(PlanRecurrence::Annually),
        _ => Err(unknown("plan recurrence", value)),
    }
}

fn parse_admission(value: &str) -> ApiResult<AdmissionPolicy> {
    match value {
        "OPEN" => Ok(AdmissionPolicy::Open),
        "APPROVAL_REQUIRED" => Ok(AdmissionPolicy::ApprovalRequired),
        _ => Err(unknown("admission policy", value)),
    }
}

fn unknown(label: &str, value: &str) -> ApiError {
    ApiError::unexpected(format!("{label} {value:?} is unknown"))
}
