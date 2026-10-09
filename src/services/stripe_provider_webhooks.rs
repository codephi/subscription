use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

use super::stripe_webhooks::{json_string, sha256_hex, webhook_result};
use crate::{
    dto::billing::BillingWebhookResponse,
    error::{ApiError, ApiResult},
    repositories::{
        billing_confirmation::{ConfirmationResult, ConfirmedBillingWebhook},
        database::DatabaseRepository,
        stripe::StripeConnector,
    },
    services::{billing, stripe_webhooks},
};

pub(super) async fn process_hosted_checkout_webhook(
    repository: &DatabaseRepository,
    connection_id: Uuid,
    event: serde_json::Value,
    payload: &[u8],
) -> ApiResult<BillingWebhookResponse> {
    let event_id = json_string(&event, "/id")?;
    let session = event.pointer("/data/object").ok_or_else(|| {
        ApiError::unprocessable(
            "stripe_checkout_session_missing",
            "checkout.session.completed has no session object",
        )
    })?;
    if session
        .get("payment_status")
        .and_then(serde_json::Value::as_str)
        != Some("paid")
    {
        return Ok(webhook_result("IGNORED"));
    }
    let session_id = json_string(session, "/id")?;
    let checkout_ref = json_string(session, "/client_reference_id")?;
    let checkout_id = Uuid::parse_str(&checkout_ref).map_err(|error| {
        ApiError::unprocessable(
            "invalid_hosted_checkout_reference",
            format!("checkout reference {checkout_ref:?} must be a UUID: {error}"),
        )
    })?;
    let customer_id = json_string(session, "/customer")?;
    let checkout = repository.checkout(checkout_id, None).await?;
    let integration = repository
        .integration_secrets(checkout.account_id, connection_id)
        .await?;
    let managed = integration.secret_reference.starts_with("v1:");
    let api_secret = billing::resolve_connection_secret(
        repository,
        checkout.account_id,
        connection_id,
        "stripe_api",
        &integration.secret_reference,
        managed,
    )?;
    let connected_account = integration
        .external_account_reference
        .starts_with("acct_")
        .then_some(integration.external_account_reference);
    let connector = StripeConnector::new(api_secret, connected_account);
    let expected_mode = if checkout.checkout_kind == crate::dto::checkouts::CheckoutKind::Initial {
        "subscription"
    } else {
        "payment"
    };
    let receipt = connector
        .retrieve_hosted_payment(
            &session_id,
            &customer_id,
            &checkout_id.to_string(),
            expected_mode,
        )
        .await
        .map_err(|error| {
            ApiError::external("stripe_hosted_payment_retrieval_failed", error.to_string())
        })?;
    let snapshot = repository
        .confirm_hosted_payment(
            checkout_id,
            &session_id,
            &receipt.payment_intent_id,
            &receipt.payment_method_id,
            receipt.saved_for_future || receipt.subscription_id.is_some(),
            receipt.amount_minor,
            &receipt.currency,
            receipt.card.as_ref(),
        )
        .await?;
    let recurring = receipt.subscription_id.is_some();
    if recurring {
        repository
            .record_provider_subscription(
                checkout_id,
                receipt.subscription_id.as_deref().unwrap_or_default(),
                &customer_id,
                &receipt.payment_method_id,
                stripe_time(receipt.period_start),
                stripe_time(receipt.period_end),
            )
            .await?;
        return Ok(webhook_result("OBSERVED"));
    }
    if !receipt.saved_for_future {
        connector
            .detach_payment_method(&receipt.payment_method_id)
            .await
            .map_err(|error| {
                ApiError::external("stripe_payment_method_cleanup_failed", error.to_string())
            })?;
    }
    let created = event
        .get("created")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let occurred_at = Utc.timestamp_opt(created, 0).single().ok_or_else(|| {
        ApiError::unprocessable(
            "invalid_stripe_event_time",
            format!("Stripe event {event_id} has invalid created value {created}"),
        )
    })?;
    let webhook = ConfirmedBillingWebhook {
        provider: "STRIPE".to_string(),
        provider_event_id: event_id,
        event_type: "payment.confirmed".to_string(),
        payload_sha256: sha256_hex(payload),
        collection_request_id: snapshot.collection_request_id,
        provider_payment_id: receipt.payment_intent_id,
        amount_minor: snapshot.amount_minor,
        currency: snapshot.currency,
        occurred_at,
    };
    match billing::apply_confirmed_webhook(repository, &webhook).await {
        Ok(outcome) => Ok(webhook_result(match outcome.result {
            ConfirmationResult::Applied => "APPLIED",
            ConfirmationResult::Duplicate => "DUPLICATE",
            ConfirmationResult::Rejected => "REJECTED",
        })),
        Err(error) if error.code() == "collection_request_not_found" => {
            billing::record_unmatched_payment(repository, connection_id, &webhook).await?;
            Ok(webhook_result("UNMATCHED"))
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn process_paid_invoice(
    repository: &DatabaseRepository,
    connection_id: Uuid,
    event: serde_json::Value,
    payload: &[u8],
) -> ApiResult<BillingWebhookResponse> {
    let object = event.pointer("/data/object").ok_or_else(|| {
        ApiError::unprocessable(
            "stripe_invoice_missing",
            "invoice.paid has no invoice object",
        )
    })?;
    let subscription_id = object
        .pointer("/subscription")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ApiError::unprocessable(
                "stripe_invoice_subscription_missing",
                "paid invoice has no subscription reference",
            )
        })?;
    if repository
        .provider_subscription_connection(subscription_id)
        .await?
        != Some(connection_id)
    {
        return Ok(webhook_result("IGNORED"));
    }
    let invoice_id = json_string(object, "/id")?;
    let provider_payment_id = object
        .pointer("/payment_intent")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            object
                .pointer("/payments/data/0/payment/payment_intent")
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or(&invoice_id)
        .to_string();
    let amount = object
        .get("amount_paid")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let currency = json_string(object, "/currency")?.to_ascii_uppercase();
    let created = event
        .get("created")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let occurred_at = Utc.timestamp_opt(created, 0).single().ok_or_else(|| {
        ApiError::unprocessable(
            "invalid_stripe_event_time",
            format!("Stripe invoice event has invalid created value {created}"),
        )
    })?;
    let collection_id = repository
        .create_provider_renewal_collection(
            subscription_id,
            &invoice_id,
            &provider_payment_id,
            amount,
            &currency,
            occurred_at,
        )
        .await?;
    let period_end = object
        .pointer("/lines/data/0/period/end")
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| Utc.timestamp_opt(value, 0).single())
        .ok_or_else(|| {
            ApiError::unprocessable(
                "stripe_invoice_period_missing",
                format!("invoice {invoice_id} has no period end"),
            )
        })?;
    let webhook = ConfirmedBillingWebhook {
        provider: "STRIPE".to_string(),
        provider_event_id: json_string(&event, "/id")?,
        event_type: "payment.confirmed".to_string(),
        payload_sha256: sha256_hex(payload),
        collection_request_id: collection_id,
        provider_payment_id,
        amount_minor: amount,
        currency,
        occurred_at,
    };
    let outcome =
        billing::apply_provider_confirmed_webhook(repository, &webhook, period_end).await?;
    Ok(webhook_result(match outcome.result {
        ConfirmationResult::Applied => "APPLIED",
        ConfirmationResult::Duplicate => "DUPLICATE",
        ConfirmationResult::Rejected => "REJECTED",
    }))
}

pub(super) async fn process_stripe_webhook_for_invoice_scope(
    repository: &DatabaseRepository,
    signature: &str,
    payload: &[u8],
    shared_secret: &str,
) -> ApiResult<BillingWebhookResponse> {
    let event: serde_json::Value =
        serde_json::from_slice(payload).map_err(ApiError::invalid_json)?;
    let subscription_id = event
        .pointer("/data/object/subscription")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ApiError::unprocessable(
                "stripe_invoice_subscription_missing",
                "paid invoice has no subscription reference",
            )
        })?;
    let Some(connection_id) = repository
        .provider_subscription_connection(subscription_id)
        .await?
    else {
        return Ok(webhook_result("IGNORED"));
    };
    let configuration = repository
        .billing_connector_configuration(connection_id)
        .await?;
    let secret = billing::resolve_connection_secret(
        repository,
        configuration.account_id,
        connection_id,
        "stripe_webhook",
        &configuration.webhook_secret_reference,
        configuration.managed,
    )?;
    if secret != shared_secret {
        return Err(ApiError::unauthorized(
            "stripe_webhook_secret_mismatch",
            "shared webhook secret does not match the subscription connection",
        ));
    }
    stripe_webhooks::process_stripe_webhook(repository, connection_id, signature, payload).await
}

pub(super) fn stripe_time(timestamp: Option<i64>) -> Option<DateTime<Utc>> {
    timestamp.and_then(|value| Utc.timestamp_opt(value, 0).single())
}
