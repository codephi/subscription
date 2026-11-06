use uuid::Uuid;

use crate::{
    dto::admin_queries::{
        AccountPageResponse, AccountProjectionResponse, AdminPageQuery, CreateAccountRequest,
        CustomerPlanPageResponse,
    },
    dto::events::AccountEventResponse,
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

/// Create a account from the administrative panel; e.g. `create_account(repo, request).await`.
pub async fn create_account(
    repository: &DatabaseRepository,
    mut request: CreateAccountRequest,
) -> ApiResult<AccountProjectionResponse> {
    request.actor_reference = request.actor_reference.trim().to_string();
    validate_account_actor(&request.actor_reference)?;
    let account = repository.create_account(&request).await?;
    crate::services::billing_integrations::provision_account_defaults(
        repository,
        account.account_id,
    )
    .await?;
    Ok(account)
}

fn validate_account_actor(actor_reference: &str) -> ApiResult<()> {
    if actor_reference.is_empty() || actor_reference.len() > 255 {
        return Err(ApiError::unprocessable(
            "invalid_actor_reference",
            format!("actor_reference {actor_reference:?} must contain 1 to 255 characters"),
        ));
    }
    Ok(())
}

/// List account projections for administration; e.g. `list_accounts(&repo, query).await`.
pub async fn list_accounts(
    repository: &DatabaseRepository,
    query: AdminPageQuery,
) -> ApiResult<AccountPageResponse> {
    repository
        .list_account_projections(query.cursor, page_limit(query.limit)?)
        .await
}

/// Read one account projection; e.g. `get_account(&repo, id).await`.
pub async fn get_account(
    repository: &DatabaseRepository,
    id: Uuid,
) -> ApiResult<AccountProjectionResponse> {
    repository.find_account_projection(id).await
}

/// Terminate a account from administration; e.g. `terminate_account(repo, id).await`.
pub async fn terminate_account(
    repository: &DatabaseRepository,
    id: Uuid,
) -> ApiResult<AccountEventResponse> {
    repository.terminate_account(id).await
}

/// List a account's customer plans; e.g. `list_customer_plans(&repo, id, query).await`.
pub async fn list_customer_plans(
    repository: &DatabaseRepository,
    id: Uuid,
    query: AdminPageQuery,
) -> ApiResult<CustomerPlanPageResponse> {
    repository.find_account_projection(id).await?;
    repository
        .list_account_customer_plans(id, query.cursor, page_limit(query.limit)?)
        .await
}

fn page_limit(limit: Option<u16>) -> ApiResult<i64> {
    let limit = limit.unwrap_or(20);
    if (1..=100).contains(&limit) {
        return Ok(i64::from(limit));
    }
    Err(ApiError::unprocessable(
        "invalid_admin_page_limit",
        format!("limit {limit} must be between 1 and 100"),
    ))
}

#[cfg(test)]
mod tests {
    use super::{page_limit, validate_account_actor};

    #[test]
    fn admin_page_limit_is_bounded() {
        assert_eq!(page_limit(None).unwrap(), 20);
        assert_eq!(page_limit(Some(100)).unwrap(), 100);
        assert!(page_limit(Some(0)).is_err());
        assert!(page_limit(Some(101)).is_err());
    }

    #[test]
    fn account_actor_reference_must_be_present_and_bounded() {
        assert!(validate_account_actor("operator@example.com").is_ok());
        assert!(validate_account_actor("").is_err());
        assert!(validate_account_actor(&"x".repeat(256)).is_err());
    }
}
