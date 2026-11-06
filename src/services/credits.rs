use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    dto::credits::{
        AccountBillingConfigResponse, AccountTransactionResponse,
        CreditLedgerReconciliationResponse, CustomerWalletStatementResponse, DirectCreditRequest,
        DirectCreditResponse, StatementQuery, UpdateAccountBillingConfigRequest,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

pub async fn grant_direct_credit(
    repository: &DatabaseRepository,
    account_id: Uuid,
    idempotency_key: &str,
    request: DirectCreditRequest,
) -> ApiResult<DirectCreditResponse> {
    validate_direct_credit(idempotency_key, &request)?;
    let request_hash = canonical_request_hash(&request)?;
    repository
        .insert_direct_credit(account_id, idempotency_key, &request_hash, &request)
        .await
}

pub async fn find_transaction(
    repository: &DatabaseRepository,
    account_id: Uuid,
    transaction_id: &str,
) -> ApiResult<AccountTransactionResponse> {
    validate_identifier("transaction_id", transaction_id)?;
    repository
        .find_customer_wallet_transaction(account_id, transaction_id)
        .await
}

pub async fn statement(
    repository: &DatabaseRepository,
    account_id: Uuid,
    query: StatementQuery,
) -> ApiResult<CustomerWalletStatementResponse> {
    let cursor = query.cursor.as_deref().map(parse_cursor).transpose()?;
    let limit = i64::from(query.limit.unwrap_or(50));
    if !(1..=100).contains(&limit) {
        return Err(ApiError::unprocessable(
            "invalid_page_limit",
            format!("statement limit {limit} must be between 1 and 100"),
        ));
    }
    repository
        .list_customer_wallet_entries(account_id, cursor, limit)
        .await
}

pub async fn reconcile(
    repository: &DatabaseRepository,
    account_id: Uuid,
) -> ApiResult<CreditLedgerReconciliationResponse> {
    repository.reconcile_credit_ledger(account_id).await
}

pub async fn get_billing_config(
    repository: &DatabaseRepository,
    account_id: Uuid,
) -> ApiResult<AccountBillingConfigResponse> {
    repository.find_billing_config(account_id).await
}

pub async fn update_billing_config(
    repository: &DatabaseRepository,
    account_id: Uuid,
    request: UpdateAccountBillingConfigRequest,
) -> ApiResult<AccountBillingConfigResponse> {
    repository.update_billing_config(account_id, &request).await
}

fn validate_direct_credit(key: &str, request: &DirectCreditRequest) -> ApiResult<()> {
    validate_identifier("Idempotency-Key", key)?;
    validate_identifier("transaction_id", &request.transaction_id)?;
    if request.credit_units.value() <= 0 {
        return Err(ApiError::unprocessable(
            "invalid_credit_units",
            format!(
                "credit_units {} must be a positive signed 64-bit integer",
                request.credit_units.value()
            ),
        ));
    }
    validate_optional_text("description", request.description.as_deref(), 500)?;
    validate_optional_text(
        "external_reference",
        request.external_reference.as_deref(),
        500,
    )?;
    if let Some(metadata) = &request.metadata {
        validate_metadata(metadata)?;
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> ApiResult<()> {
    if (1..=255).contains(&value.len()) && value.trim() == value {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_identifier",
        format!("{label} {value:?} must contain 1 to 255 characters without edge whitespace"),
    ))
}

fn validate_optional_text(label: &str, value: Option<&str>, maximum: usize) -> ApiResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if (1..=maximum).contains(&value.len()) && value.trim() == value {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_credit_context",
        format!("{label} {value:?} must contain 1 to {maximum} characters without edge whitespace"),
    ))
}

fn validate_metadata(metadata: &Value) -> ApiResult<()> {
    let encoded = serde_json::to_vec(metadata).map_err(ApiError::serialization)?;
    if !metadata.is_object() || encoded.len() > 4096 || !metadata_shape_is_valid(metadata, 0) {
        return Err(ApiError::unprocessable(
            "invalid_metadata",
            format!(
                "metadata of {} bytes must be an object up to 4096 bytes, 32 keys, depth 3, and scalar values",
                encoded.len()
            ),
        ));
    }
    Ok(())
}

fn metadata_shape_is_valid(value: &Value, depth: usize) -> bool {
    match value {
        Value::Object(map) => {
            depth < 3
                && map.len() <= 32
                && map
                    .values()
                    .all(|nested| metadata_shape_is_valid(nested, depth + 1))
        }
        Value::Array(_) => false,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
    }
}

fn canonical_request_hash(request: &DirectCreditRequest) -> ApiResult<String> {
    let bytes = serde_json::to_vec(request).map_err(ApiError::serialization)?;
    let mut digest = Sha256::new();
    digest.update(bytes);
    Ok(format!("{:#x}", digest.finalize()))
}

fn parse_cursor(value: &str) -> ApiResult<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|cursor| *cursor > 0)
        .ok_or_else(|| {
            ApiError::unprocessable(
                "invalid_cursor",
                format!("statement cursor {value:?} must be a positive decimal integer"),
            )
        })
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{canonical_request_hash, parse_cursor, validate_direct_credit};
    use crate::dto::credits::DirectCreditRequest;

    #[test]
    fn direct_credit_validation_rejects_zero_and_nested_arrays() {
        let mut request = request(json!({"source":"test"}));
        request.credit_units = crate::dto::units::CreditUnits::new(0);
        assert_eq!(
            validate_direct_credit("credit-key", &request)
                .expect_err("zero credit")
                .code(),
            "invalid_credit_units"
        );
        request.credit_units = crate::dto::units::CreditUnits::new(1);
        request.metadata = Some(json!({"values":[1,2]}));
        assert_eq!(
            validate_direct_credit("credit-key", &request)
                .expect_err("array metadata")
                .code(),
            "invalid_metadata"
        );
    }

    #[test]
    fn direct_credit_hash_is_stable_for_metadata_key_order() {
        let first = request(json!({"b":2,"a":1}));
        let second = request(json!({"a":1,"b":2}));
        assert_eq!(
            canonical_request_hash(&first).expect("first hash"),
            canonical_request_hash(&second).expect("second hash")
        );
    }

    #[test]
    fn direct_credit_rejects_negative_units_before_persistence() {
        let mut credit = request(json!({}));
        for invalid in [i64::MIN, -1, 0] {
            credit.credit_units = crate::dto::units::CreditUnits::new(invalid);
            assert_eq!(
                validate_direct_credit("negative-credit-key", &credit)
                    .expect_err("credit must be positive")
                    .code(),
                "invalid_credit_units"
            );
        }
    }

    #[test]
    fn statement_cursor_requires_a_positive_decimal_integer() {
        assert_eq!(parse_cursor("12").expect("cursor"), 12);
        assert_eq!(
            parse_cursor("0").expect_err("zero cursor").code(),
            "invalid_cursor"
        );
        assert_eq!(
            parse_cursor("1e2").expect_err("scientific cursor").code(),
            "invalid_cursor"
        );
    }

    fn request(metadata: Value) -> DirectCreditRequest {
        serde_json::from_value(json!({
            "transaction_id":"transaction-1","credit_units":"10",
            "external_reference":"order-1","description":"Test credit","metadata":metadata
        }))
        .expect("direct credit request")
    }
}
