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
    let secret = billing::resolve_secret(&configuration.webhook_secret_reference)?;
    verify_stripe_signature(signature, payload, &secret, Utc::now())?;
    let event: StripeEvent = serde_json::from_slice(payload).map_err(ApiError::invalid_json)?;
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
