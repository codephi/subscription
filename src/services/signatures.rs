use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{ApiError, ApiResult};

const SIGNATURE_TOLERANCE_SECONDS: i64 = 300;

pub fn verify_signed_body(
    secret: &str,
    timestamp: &str,
    signature: &str,
    body: &[u8],
) -> ApiResult<()> {
    let timestamp_seconds = parse_timestamp(timestamp)?;
    validate_timestamp(timestamp_seconds, current_timestamp()?)?;
    let signature_bytes = decode_signature(signature)?;
    let mut mac = build_mac(secret)?;
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    mac.verify_slice(&signature_bytes)
        .map_err(|_| ApiError::unauthorized("invalid_webhook_signature", "signature mismatch"))
}

pub fn sign_body(secret: &str, timestamp: i64, body: &[u8]) -> ApiResult<String> {
    let timestamp = timestamp.to_string();
    let mut mac = build_mac(secret)?;
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    Ok(format!(
        "v1={}",
        general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    ))
}

pub fn current_timestamp() -> ApiResult<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ApiError::unexpected(error.to_string()))?;
    i64::try_from(duration.as_secs()).map_err(|error| ApiError::unexpected(error.to_string()))
}

fn parse_timestamp(timestamp: &str) -> ApiResult<i64> {
    timestamp.parse::<i64>().map_err(|_| {
        ApiError::unauthorized(
            "invalid_webhook_timestamp",
            format!("timestamp {timestamp:?} must be Unix seconds"),
        )
    })
}

fn validate_timestamp(timestamp: i64, now: i64) -> ApiResult<()> {
    if (now - timestamp).abs() <= SIGNATURE_TOLERANCE_SECONDS {
        return Ok(());
    }
    Err(ApiError::unauthorized(
        "expired_webhook_signature",
        format!("timestamp {timestamp} must be within {SIGNATURE_TOLERANCE_SECONDS} seconds"),
    ))
}

fn decode_signature(signature: &str) -> ApiResult<Vec<u8>> {
    let encoded = signature.strip_prefix("v1=").ok_or_else(|| {
        ApiError::unauthorized(
            "invalid_webhook_signature",
            format!("signature {signature:?} must start with v1="),
        )
    })?;
    general_purpose::STANDARD.decode(encoded).map_err(|_| {
        ApiError::unauthorized(
            "invalid_webhook_signature",
            format!("signature {signature:?} must contain base64"),
        )
    })
}

fn build_mac(secret: &str) -> ApiResult<Hmac<Sha256>> {
    Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|error| ApiError::unexpected(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{sign_body, verify_signed_body};

    #[test]
    fn signed_body_round_trips() {
        let timestamp = super::current_timestamp().expect("current timestamp");
        let signature = sign_body("secret", timestamp, b"payload").expect("signature");
        verify_signed_body("secret", &timestamp.to_string(), &signature, b"payload")
            .expect("signature must verify");
    }

    #[test]
    fn signature_rejects_changed_payload() {
        let timestamp = super::current_timestamp().expect("current timestamp");
        let signature = sign_body("secret", timestamp, b"payload").expect("signature");
        let error = verify_signed_body("secret", &timestamp.to_string(), &signature, b"changed")
            .expect_err("changed payload must fail");
        assert_eq!(error.code(), "invalid_webhook_signature");
    }

    #[test]
    fn signature_rejects_expired_timestamp() {
        let timestamp = super::current_timestamp().expect("current timestamp") - 301;
        let signature = sign_body("secret", timestamp, b"payload").expect("signature");
        let error = verify_signed_body("secret", &timestamp.to_string(), &signature, b"payload")
            .expect_err("expired signature must fail");
        assert_eq!(error.code(), "expired_webhook_signature");
    }
}
