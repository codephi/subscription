use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::{error::ApiResult, repositories::credential_vault::CredentialVault};

#[derive(Clone)]
pub struct DatabaseRepository {
    pool: PgPool,
    credential_vault: Option<CredentialVault>,
}

impl DatabaseRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            credential_vault: None,
        }
    }

    pub fn with_credential_vault(mut self) -> ApiResult<Self> {
        self.credential_vault = CredentialVault::from_environment()?;
        Ok(self)
    }

    pub fn credential_vault(&self) -> ApiResult<&CredentialVault> {
        self.credential_vault.as_ref().ok_or_else(|| {
            crate::error::ApiError::service_unavailable(
                "billing_credential_key_unavailable",
                "BILLING_CREDENTIAL_ENCRYPTION_KEY must be configured to manage Stripe credentials",
            )
        })
    }

    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    pub async fn current_time(&self) -> ApiResult<DateTime<Utc>> {
        Ok(sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&self.pool)
            .await?)
    }
}
