use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    repositories::{
        billing_renewal::renew_paid_customer_plan, credits::lock_active_customer_wallet,
        database::DatabaseRepository, plan_cycles::load_locked_plan,
        plan_writes::activate_customer_plan,
    },
};

#[derive(Clone, Debug, Serialize)]
pub struct ConfirmedBillingWebhook {
    pub provider: String,
    pub provider_event_id: String,
    pub event_type: String,
    pub payload_sha256: String,
    pub collection_request_id: Uuid,
    pub provider_payment_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmationResult {
    Applied,
    Duplicate,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfirmationOutcome {
    pub result: ConfirmationResult,
    pub customer_plan_cycle_id: Option<Uuid>,
}

impl DatabaseRepository {
    pub async fn apply_payment_confirmation(
        &self,
        webhook: &ConfirmedBillingWebhook,
        period_end: Option<DateTime<Utc>>,
    ) -> ApiResult<ConfirmationOutcome> {
        let mut transaction = self.pool().begin().await?;
        if !insert_webhook_inbox(&mut transaction, webhook).await? {
            validate_duplicate_payload(&mut transaction, webhook).await?;
            transaction.commit().await?;
            return Ok(duplicate_outcome());
        }
        let workspace_id = find_collection_workspace(&mut transaction, webhook).await?;
        let wallet = lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        let context = lock_confirmation_context(&mut transaction, webhook).await?;
        validate_payment_details(webhook, &context)?;
        if context.request_status == "PAID" {
            mark_webhook(&mut transaction, webhook, "DUPLICATE").await?;
            transaction.commit().await?;
            return Ok(duplicate_outcome());
        }
        if matches!(
            context.request_status.as_str(),
            "EXPIRED" | "EXHAUSTED" | "CANCELED" | "UNMATCHED"
        ) {
            mark_webhook(&mut transaction, webhook, "REJECTED").await?;
            transaction.commit().await?;
            return Ok(rejected_outcome());
        }
        validate_pending_confirmation(webhook, &context)?;
        let plan = load_locked_plan(&mut transaction, context.plan_version_id).await?;
        validate_plan_and_customer(webhook, &context, &plan)?;
        let cycle = if context.request_kind == "RENEWAL" {
            renew_paid_customer_plan(
                &mut transaction,
                &wallet,
                workspace_id,
                context.customer_plan_id,
                &plan,
                webhook.occurred_at,
                period_end,
                &context.transaction_id,
            )
            .await?
        } else if context.request_kind == "RENEWAL_REGULARIZATION" {
            crate::repositories::billing_renewal::regularize_paid_customer_plan(
                &mut transaction,
                &wallet,
                workspace_id,
                context.customer_plan_id,
                &plan,
                webhook.occurred_at,
                period_end,
                &context.transaction_id,
            )
            .await?
        } else {
            activate_customer_plan(
                &mut transaction,
                &wallet,
                workspace_id,
                context.customer_plan_id,
                &plan,
                webhook.occurred_at,
                period_end,
                &context.transaction_id,
            )
            .await?
        };
        finalize_confirmation(
            &mut transaction,
            webhook,
            &context,
            cycle.customer_plan_cycle_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(ConfirmationOutcome {
            result: ConfirmationResult::Applied,
            customer_plan_cycle_id: Some(cycle.customer_plan_cycle_id),
        })
    }
}

struct ConfirmationContext {
    billing_payment_id: Uuid,
    customer_plan_id: Uuid,
    plan_version_id: Uuid,
    customer_plan_version_id: Uuid,
    request_kind: String,
    granted_credit_units: i64,
    request_status: String,
    activation_status: String,
    commercial_status: String,
    renewal_status: String,
    payment_expires_at: DateTime<Utc>,
    provider: String,
    provider_payment_id: Option<String>,
    amount_minor: i64,
    currency: String,
    transaction_id: String,
}

async fn insert_webhook_inbox(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<bool> {
    let result = sqlx::query(
        "INSERT INTO billing_webhook_inbox (billing_webhook_inbox_id,provider,provider_event_id, \
         event_type,payload_sha256,payload) VALUES ($1,$2,$3,$4,$5,$6) \
         ON CONFLICT (provider,provider_event_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&webhook.provider)
    .bind(&webhook.provider_event_id)
    .bind(&webhook.event_type)
    .bind(&webhook.payload_sha256)
    .bind(serde_json::to_value(webhook).map_err(ApiError::serialization)?)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn validate_duplicate_payload(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<()> {
    let stored_hash: String = sqlx::query_scalar(
        "SELECT payload_sha256 FROM billing_webhook_inbox WHERE provider=$1 AND provider_event_id=$2",
    )
    .bind(&webhook.provider)
    .bind(&webhook.provider_event_id)
    .fetch_one(&mut **transaction)
    .await?;
    if stored_hash == webhook.payload_sha256 {
        return Ok(());
    }
    Err(ApiError::conflict(
        "webhook_event_payload_mismatch",
        format!(
            "provider event {} already exists with payload hash {stored_hash}, not {}",
            webhook.provider_event_id, webhook.payload_sha256
        ),
    ))
}

async fn find_collection_workspace(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<Uuid> {
    sqlx::query_scalar(
        "SELECT workspace_id FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(webhook.collection_request_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| missing_collection(webhook.collection_request_id))
}

async fn lock_confirmation_context(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<ConfirmationContext> {
    let row = sqlx::query(
        "SELECT cr.customer_plan_id,cr.plan_version_id,cr.request_kind,cr.granted_credit_units, \
         cr.status request_status,cr.payment_expires_at,cr.amount_minor,cr.currency,cr.transaction_id, \
         cp.plan_version_id customer_plan_version_id,cp.activation_status,cp.commercial_status,cp.renewal_status, \
         bp.billing_payment_id,bp.provider,bp.provider_payment_id \
         FROM collection_requests cr JOIN customer_plans cp ON cp.customer_plan_id=cr.customer_plan_id \
         JOIN billing_payments bp ON bp.collection_request_id=cr.collection_request_id \
         JOIN collection_attempts ca ON ca.collection_attempt_id=bp.collection_attempt_id \
         WHERE cr.collection_request_id=$1 FOR UPDATE OF cr,cp,bp,ca",
    )
    .bind(webhook.collection_request_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| missing_collection(webhook.collection_request_id))?;
    Ok(context_from_row(&row))
}

fn context_from_row(row: &sqlx::postgres::PgRow) -> ConfirmationContext {
    ConfirmationContext {
        billing_payment_id: row.get("billing_payment_id"),
        customer_plan_id: row.get("customer_plan_id"),
        plan_version_id: row.get("plan_version_id"),
        customer_plan_version_id: row.get("customer_plan_version_id"),
        request_kind: row.get("request_kind"),
        granted_credit_units: row.get("granted_credit_units"),
        request_status: row.get("request_status"),
        activation_status: row.get("activation_status"),
        commercial_status: row.get("commercial_status"),
        renewal_status: row.get("renewal_status"),
        payment_expires_at: row.get("payment_expires_at"),
        provider: row.get("provider"),
        provider_payment_id: row.get("provider_payment_id"),
        amount_minor: row.get("amount_minor"),
        currency: row.get("currency"),
        transaction_id: row.get("transaction_id"),
    }
}

fn validate_payment_details(
    webhook: &ConfirmedBillingWebhook,
    context: &ConfirmationContext,
) -> ApiResult<()> {
    let payment_id_mismatch = context
        .provider_payment_id
        .as_deref()
        .is_some_and(|payment_id| payment_id != webhook.provider_payment_id);
    if context.provider != webhook.provider
        || payment_id_mismatch
        || context.amount_minor != webhook.amount_minor
        || context.currency != webhook.currency
    {
        return Err(invalid_confirmation(
            webhook,
            "provider payment, amount, and currency must match",
        ));
    }
    Ok(())
}

fn validate_pending_confirmation(
    webhook: &ConfirmedBillingWebhook,
    context: &ConfirmationContext,
) -> ApiResult<()> {
    if !matches!(
        context.request_status.as_str(),
        "COLLECTING" | "PENDING_PAYMENT"
    ) {
        return Err(invalid_confirmation(
            webhook,
            "request must be awaiting payment",
        ));
    }
    if webhook.occurred_at < context.payment_expires_at {
        return Ok(());
    }
    Err(invalid_confirmation(
        webhook,
        "confirmation must precede payment_expires_at",
    ))
}

fn validate_plan_and_customer(
    webhook: &ConfirmedBillingWebhook,
    context: &ConfirmationContext,
    plan: &crate::repositories::plan_rows::PlanRecord,
) -> ApiResult<()> {
    if plan.response.revoked_at.is_some() {
        return Err(invalid_confirmation(
            webhook,
            "subscription plan must not be revoked",
        ));
    }
    if context.customer_plan_version_id != context.plan_version_id
        || plan.response.commercial_model != crate::dto::plans::CommercialModel::Paid
        || context.granted_credit_units != plan.response.granted_credit_units.value()
    {
        return Err(invalid_confirmation(
            webhook,
            "request must match the paid customer plan and published credit grant",
        ));
    }
    let initial = context.request_kind == "INITIAL"
        && context.commercial_status == "ACTIVE"
        && context.activation_status == "PENDING_INITIAL_PAYMENT";
    let renewal = context.request_kind == "RENEWAL"
        && context.commercial_status == "ACTIVE_PAID"
        && context.activation_status == "ACTIVATED"
        && context.renewal_status == "CURRENT";
    let regularization = context.request_kind == "RENEWAL_REGULARIZATION"
        && context.commercial_status == "PAST_DUE"
        && context.activation_status == "ACTIVATED"
        && context.renewal_status == "RENEWAL_INACTIVE";
    if initial || renewal || regularization {
        return Ok(());
    }
    Err(invalid_confirmation(
        webhook,
        "customer plan state must match initial activation or paid renewal",
    ))
}

async fn finalize_confirmation(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
    context: &ConfirmationContext,
    cycle_id: Uuid,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE customer_plans SET commercial_status='ACTIVE_PAID',activation_status='ACTIVATED', \
         renewal_status='CURRENT',version=version+1 WHERE customer_plan_id=$1",
    )
    .bind(context.customer_plan_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE billing_payments SET state='CONFIRMED',confirmed_at=$2, \
         provider_payment_id=COALESCE(provider_payment_id,$3) WHERE billing_payment_id=$1",
    )
    .bind(context.billing_payment_id)
    .bind(webhook.occurred_at)
    .bind(&webhook.provider_payment_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("UPDATE collection_attempts SET status='SUCCEEDED',finished_at=$2 WHERE collection_request_id=$1")
        .bind(webhook.collection_request_id).bind(webhook.occurred_at)
        .execute(&mut **transaction).await?;
    sqlx::query("UPDATE collection_requests SET status='PAID' WHERE collection_request_id=$1")
        .bind(webhook.collection_request_id)
        .execute(&mut **transaction)
        .await?;
    insert_billing_credit_reference(transaction, webhook, context, cycle_id).await?;
    mark_webhook(transaction, webhook, "APPLIED").await?;
    insert_confirmation_event(transaction, webhook, context.customer_plan_id).await
}

async fn insert_billing_credit_reference(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
    context: &ConfirmationContext,
    cycle_id: Uuid,
) -> ApiResult<()> {
    let entry_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT customer_wallet_entry_id FROM wallet_transaction_references \
         WHERE reference_kind='CUSTOMER_PLAN_CYCLE' AND customer_plan_cycle_id=$1",
    )
    .bind(cycle_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(entry_id) = entry_id else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO billing_credit_grant_references (billing_credit_grant_reference_id, \
         customer_wallet_entry_id,collection_request_id,billing_payment_id) VALUES ($1,$2,$3,$4)",
    )
    .bind(Uuid::new_v4())
    .bind(entry_id)
    .bind(webhook.collection_request_id)
    .bind(context.billing_payment_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn mark_webhook(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
    result: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE billing_webhook_inbox SET processed_at=clock_timestamp(),result=$3 \
         WHERE provider=$1 AND provider_event_id=$2",
    )
    .bind(&webhook.provider)
    .bind(&webhook.provider_event_id)
    .bind(result)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_confirmation_event(
    transaction: &mut Transaction<'_, Postgres>,
    webhook: &ConfirmedBillingWebhook,
    customer_plan_id: Uuid,
) -> ApiResult<()> {
    let (workspace_id, correlation_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT workspace_id,correlation_id FROM collection_requests WHERE collection_request_id=$1",
    )
    .bind(webhook.collection_request_id)
    .fetch_one(&mut **transaction)
    .await?;
    let event_id = Uuid::new_v4();
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(aggregate_sequence),0)+1 FROM outbox_events \
         WHERE aggregate_type='collection_request' AND aggregate_id=$1",
    )
    .bind(webhook.collection_request_id)
    .fetch_one(&mut **transaction)
    .await?;
    let payload = json!({"billing_event_id":event_id,"event_type":"payment.confirmed","schema_version":1,
        "occurred_at":webhook.occurred_at,"workspace_id":workspace_id,"correlation_id":correlation_id,
        "collection_request_id":webhook.collection_request_id,"customer_plan_id":customer_plan_id,
        "provider_payment_id":webhook.provider_payment_id});
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         workspace_id,correlation_id,payload) VALUES \
         ($1,'payment.confirmed','collection_request',$2,$3,$4,$5,$6)",
    )
    .bind(event_id)
    .bind(webhook.collection_request_id)
    .bind(sequence)
    .bind(workspace_id)
    .bind(correlation_id)
    .bind(payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn invalid_confirmation(webhook: &ConfirmedBillingWebhook, expected: &str) -> ApiError {
    ApiError::conflict(
        "billing_confirmation_mismatch",
        format!(
            "provider event {} for collection {} is invalid: {expected}",
            webhook.provider_event_id, webhook.collection_request_id
        ),
    )
}

fn missing_collection(collection_request_id: Uuid) -> ApiError {
    ApiError::not_found(
        "collection_request_not_found",
        format!("collection request {collection_request_id} does not exist"),
    )
}

fn duplicate_outcome() -> ConfirmationOutcome {
    ConfirmationOutcome {
        result: ConfirmationResult::Duplicate,
        customer_plan_cycle_id: None,
    }
}

fn rejected_outcome() -> ConfirmationOutcome {
    ConfirmationOutcome {
        result: ConfirmationResult::Rejected,
        customer_plan_cycle_id: None,
    }
}
