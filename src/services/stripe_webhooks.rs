use chrono::{DateTime, TimeZone, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    dto::billing::BillingWebhookResponse,
    error::{ApiError, ApiResult},
    repositories::{
        billing_confirmation::{ConfirmationResult, ConfirmedBillingWebhook},
        billing_status_webhooks::PaymentStatusWebhookResult,
        database::DatabaseRepository,
        stripe::StripeConnector,
    },
    services::billing,
};

const SIGNATURE_TOLERANCE_SECONDS: i64 = 300;

#[derive(Deserialize)]
struct StripeEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: String,
    created: i64,
    #[serde(default)]
    livemode: Option<bool>,
    #[serde(default)]
    account: Option<String>,
    data: StripeEventData,
}

#[derive(Deserialize)]
struct StripeEventData {
    object: StripePaymentIntent,
}

#[derive(Deserialize)]
struct StripePaymentIntent {
    id: String,
    amount: i64,
    currency: String,
    #[serde(default)]
    customer: Option<String>,
    #[serde(default)]
    amount_refunded: Option<i64>,
    #[serde(default)]
    payment_intent: Option<String>,
    #[serde(default)]
    metadata: StripeMetadata,
}

#[derive(Default, Deserialize)]
struct StripeMetadata {
    collection_request_id: Option<Uuid>,
}

pub async fn process_stripe_webhook(
    repository: &DatabaseRepository,
    connection_id: Uuid,
    signature: &str,
    payload: &[u8],
) -> ApiResult<BillingWebhookResponse> {
    let configuration = repository
        .billing_connector_configuration(connection_id)
        .await?;
    if configuration.provider != "STRIPE" || configuration.status != "ACTIVE" {
        return Err(ApiError::conflict(
            "billing_connection_not_usable",
            format!("billing connection {connection_id} must be an ACTIVE STRIPE connection"),
        ));
    }
    if configuration.webhook_secret_reference.is_empty() {
        return Err(ApiError::conflict(
            "billing_webhook_not_configured",
            format!("billing connection {connection_id} has no configured webhook secret"),
        ));
    }
    let secret = billing::resolve_connection_secret(
        repository,
        configuration.workspace_id,
        connection_id,
        "stripe_webhook",
        &configuration.webhook_secret_reference,
        configuration.managed,
    )?;
    verify_stripe_signature(signature, payload, &secret, Utc::now())?;
    let event_value: serde_json::Value =
        serde_json::from_slice(payload).map_err(ApiError::invalid_json)?;
    if event_value.get("type").and_then(serde_json::Value::as_str)
        == Some("checkout.session.completed")
    {
        return process_hosted_checkout_webhook(repository, connection_id, event_value, payload)
            .await;
    }
    let event: StripeEvent = serde_json::from_value(event_value).map_err(ApiError::invalid_json)?;
    if event.event_type == "charge.refunded" {
        let inserted = repository
            .record_external_refund(
                connection_id,
                &event.id,
                event
                    .data
                    .object
                    .payment_intent
                    .as_deref()
                    .unwrap_or(&event.data.object.id),
                event
                    .data
                    .object
                    .amount_refunded
                    .unwrap_or(event.data.object.amount),
                &event.data.object.currency.to_ascii_uppercase(),
                &sha256_hex(payload),
            )
            .await?;
        return Ok(BillingWebhookResponse {
            result: if inserted { "OBSERVED" } else { "DUPLICATE" }.to_string(),
        });
    }
    let webhook = normalized_confirmation(event, payload)?;
    if webhook.event_type != "payment.confirmed" {
        let result = repository.apply_payment_status_webhook(&webhook).await?;
        return Ok(BillingWebhookResponse {
            result: match result {
                PaymentStatusWebhookResult::Applied => "APPLIED",
                PaymentStatusWebhookResult::Duplicate => "DUPLICATE",
                PaymentStatusWebhookResult::Rejected => "REJECTED",
            }
            .to_string(),
        });
    }
    match billing::apply_confirmed_webhook(repository, &webhook).await {
        Ok(outcome) => Ok(BillingWebhookResponse {
            result: match outcome.result {
                ConfirmationResult::Applied => "APPLIED",
                ConfirmationResult::Duplicate => "DUPLICATE",
                ConfirmationResult::Rejected => "REJECTED",
            }
            .to_string(),
        }),
        Err(error) if error.code() == "collection_request_not_found" => {
            billing::record_unmatched_payment(repository, connection_id, &webhook).await?;
            Ok(BillingWebhookResponse {
                result: "UNMATCHED".to_string(),
            })
        }
        Err(error) => Err(error),
    }
}

pub async fn process_shared_stripe_webhook(
    repository: &DatabaseRepository,
    signature: &str,
    payload: &[u8],
    shared_secret: &str,
) -> ApiResult<BillingWebhookResponse> {
    verify_stripe_signature(signature, payload, shared_secret, Utc::now())?;
    let event: serde_json::Value =
        serde_json::from_slice(payload).map_err(ApiError::invalid_json)?;
    let event_type = event
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if event_type != "checkout.session.completed" && !is_payment_event(event_type) {
        return Ok(webhook_result("IGNORED"));
    }
    let Some(collection_id) = event
        .pointer("/data/object/metadata/collection_request_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
    else {
        return Ok(webhook_result("IGNORED"));
    };
    let Some(scope) = find_webhook_scope(repository, collection_id).await? else {
        return Ok(webhook_result("IGNORED"));
    };
    if event_type != "checkout.session.completed" {
        let typed: StripeEvent = serde_json::from_value(event).map_err(ApiError::invalid_json)?;
        validate_webhook_scope(&typed, &scope)?;
    }
    process_stripe_webhook(repository, scope.billing_connection_id, signature, payload).await
}

async fn process_hosted_checkout_webhook(
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
        .integration_secrets(checkout.workspace_id, connection_id)
        .await?;
    let managed = integration.secret_reference.starts_with("v1:");
    let api_secret = billing::resolve_connection_secret(
        repository,
        checkout.workspace_id,
        connection_id,
        "stripe_api",
        &integration.secret_reference,
        managed,
    )?;
    let connected_account = integration
        .external_account_reference
        .starts_with("acct_")
        .then_some(integration.external_account_reference);
    let receipt = StripeConnector::new(api_secret, connected_account)
        .retrieve_hosted_payment(&session_id, &customer_id, &checkout_id.to_string())
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
            receipt.amount_minor,
            &receipt.currency,
        )
        .await?;
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

fn json_string(value: &serde_json::Value, path: &str) -> ApiResult<String> {
    value
        .pointer(path)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            ApiError::unprocessable(
                "invalid_stripe_checkout_session",
                format!("Stripe field {path} must be a string"),
            )
        })
}

async fn find_webhook_scope(
    repository: &DatabaseRepository,
    collection_id: Uuid,
) -> ApiResult<Option<crate::repositories::integrations::CheckoutWebhookScope>> {
    match repository.checkout_webhook_scope(collection_id).await {
        Ok(scope) => Ok(Some(scope)),
        Err(error) if error.code() == "collection_request_not_found" => Ok(None),
        Err(error) => Err(error),
    }
}

fn validate_webhook_scope(
    event: &StripeEvent,
    scope: &crate::repositories::integrations::CheckoutWebhookScope,
) -> ApiResult<()> {
    let customer_matches =
        scope.provider_customer_reference.as_deref() == event.data.object.customer.as_deref();
    let account_matches = event
        .account
        .as_ref()
        .is_none_or(|account| scope.provider_account_reference.as_deref() == Some(account));
    if scope.provider == "STRIPE"
        && scope.environment.as_deref() == Some("TEST")
        && event.livemode == Some(false)
        && customer_matches
        && account_matches
    {
        return Ok(());
    }
    Err(ApiError::conflict(
        "billing_webhook_scope_mismatch",
        format!(
            "Stripe event {} does not match its test checkout customer and account",
            event.id
        ),
    ))
}

fn is_payment_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "payment_intent.succeeded"
            | "payment_intent.payment_failed"
            | "payment_intent.canceled"
            | "payment_intent.requires_action"
    )
}

fn webhook_result(result: &str) -> BillingWebhookResponse {
    BillingWebhookResponse {
        result: result.to_string(),
    }
}

pub fn verify_stripe_signature(
    signature: &str,
    payload: &[u8],
    secret: &str,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let (timestamp, signatures) = parse_signature(signature)?;
    if (now.timestamp() - timestamp).abs() > SIGNATURE_TOLERANCE_SECONDS {
        return Err(ApiError::unauthorized(
            "stripe_signature_expired",
            format!("Stripe signature timestamp {timestamp} must be within 300 seconds"),
        ));
    }
    let signed = [timestamp.to_string().as_bytes(), b".", payload].concat();
    let valid = signatures.iter().any(|candidate| {
        let Ok(bytes) = decode_hex(candidate) else {
            return false;
        };
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
            return false;
        };
        mac.update(&signed);
        mac.verify_slice(&bytes).is_ok()
    });
    if valid {
        return Ok(());
    }
    Err(ApiError::unauthorized(
        "stripe_signature_invalid",
        "Stripe-Signature has no valid v1 digest for the exact request body",
    ))
}

fn normalized_confirmation(
    event: StripeEvent,
    payload: &[u8],
) -> ApiResult<ConfirmedBillingWebhook> {
    let event_type = match event.event_type.as_str() {
        "payment_intent.succeeded" => "payment.confirmed",
        "payment_intent.payment_failed" | "payment_intent.canceled" => "payment.failed",
        "payment_intent.requires_action" => "payment.requires_action",
        other => other,
    };
    let occurred_at = Utc
        .timestamp_opt(event.created, 0)
        .single()
        .ok_or_else(|| {
            ApiError::unprocessable(
                "invalid_stripe_event_time",
                format!(
                    "Stripe event {} has invalid created value {}",
                    event.id, event.created
                ),
            )
        })?;
    Ok(ConfirmedBillingWebhook {
        provider: "STRIPE".to_string(),
        provider_event_id: event.id,
        event_type: event_type.to_string(),
        payload_sha256: sha256_hex(payload),
        collection_request_id: event
            .data
            .object
            .metadata
            .collection_request_id
            .unwrap_or_else(Uuid::new_v4),
        provider_payment_id: event.data.object.id,
        amount_minor: event.data.object.amount,
        currency: event.data.object.currency.to_ascii_uppercase(),
        occurred_at,
    })
}

fn parse_signature(signature: &str) -> ApiResult<(i64, Vec<&str>)> {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in signature.split(',') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        match key {
            "t" => timestamp = value.parse().ok(),
            "v1" => signatures.push(value),
            _ => {}
        }
    }
    match (timestamp, signatures.is_empty()) {
        (Some(timestamp), false) => Ok((timestamp, signatures)),
        _ => Err(ApiError::unauthorized(
            "stripe_signature_invalid",
            format!("Stripe-Signature {signature:?} must contain numeric t and v1 values"),
        )),
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, ()> {
    if !value.len().is_multiple_of(2) {
        return Err(());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| ())?;
            u8::from_str_radix(text, 16).map_err(|_| ())
        })
        .collect()
}

fn sha256_hex(payload: &[u8]) -> String {
    Sha256::digest(payload)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripe_signature_accepts_exact_payload_and_rejects_changes() {
        let now = Utc::now();
        let payload = br#"{"id":"evt_1"}"#;
        let signed = [now.timestamp().to_string().as_bytes(), b".", payload].concat();
        let mut mac = Hmac::<Sha256>::new_from_slice(b"whsec_test").unwrap();
        mac.update(&signed);
        let digest: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let header = format!("t={},v1={digest}", now.timestamp());
        verify_stripe_signature(&header, payload, "whsec_test", now).unwrap();
        assert_eq!(
            verify_stripe_signature(&header, b"{}", "whsec_test", now)
                .unwrap_err()
                .code(),
            "stripe_signature_invalid"
        );
    }

    #[test]
    fn stripe_signature_rejects_stale_timestamp() {
        let now = Utc::now();
        let error = verify_stripe_signature(
            &format!("t={},v1={}", now.timestamp() - 301, "00".repeat(32)),
            b"{}",
            "whsec_test",
            now,
        )
        .unwrap_err();
        assert_eq!(error.code(), "stripe_signature_expired");
    }
}
