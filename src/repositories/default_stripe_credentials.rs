use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::billing::DefaultStripeCredentialsResponse,
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

const DEFAULT_SCOPE_ID: Uuid = Uuid::from_u128(0);

#[derive(Clone)]
pub struct DefaultStripeSecrets {
    pub environment: String,
    pub account_reference: String,
    pub api_secret: String,
    pub webhook_secret: Option<String>,
}

impl DatabaseRepository {
    pub async fn default_stripe_credentials(&self) -> ApiResult<DefaultStripeCredentialsResponse> {
        let row =
            sqlx::query("SELECT * FROM billing_default_stripe_credentials WHERE singleton_id=1")
                .fetch_optional(&self.pool())
                .await?;
        Ok(match row {
            Some(row) => DefaultStripeCredentialsResponse {
                configured: true,
                environment: row.try_get("environment").ok(),
                account_reference: row.try_get("provider_account_reference").ok(),
                api_secret_configured: true,
                webhook_secret_configured: row
                    .try_get::<Option<String>, _>("webhook_secret_reference")
                    .ok()
                    .flatten()
                    .is_some(),
                configuration_version: row.get("configuration_version"),
            },
            None => DefaultStripeCredentialsResponse {
                configured: false,
                environment: None,
                account_reference: None,
                api_secret_configured: false,
                webhook_secret_configured: false,
                configuration_version: 0,
            },
        })
    }

    pub async fn load_default_stripe_secrets(&self) -> ApiResult<Option<DefaultStripeSecrets>> {
        let row =
            sqlx::query("SELECT * FROM billing_default_stripe_credentials WHERE singleton_id=1")
                .fetch_optional(&self.pool())
                .await?;
        let Some(row) = row else { return Ok(None) };
        let vault = self.credential_vault()?;
        let api_ciphertext: String = row.get("api_secret_reference");
        let webhook_ciphertext: Option<String> = row.try_get("webhook_secret_reference")?;
        Ok(Some(DefaultStripeSecrets {
            environment: row.get("environment"),
            account_reference: row.get("provider_account_reference"),
            api_secret: vault.open(
                DEFAULT_SCOPE_ID,
                DEFAULT_SCOPE_ID,
                "stripe_default_api",
                &api_ciphertext,
            )?,
            webhook_secret: webhook_ciphertext
                .map(|ciphertext| {
                    vault.open(
                        DEFAULT_SCOPE_ID,
                        DEFAULT_SCOPE_ID,
                        "stripe_default_webhook",
                        &ciphertext,
                    )
                })
                .transpose()?,
        }))
    }

    pub async fn save_default_stripe_credentials(
        &self,
        expected_version: i32,
        environment: &str,
        account_reference: &str,
        secret_key: Option<&str>,
        webhook_secret: Option<&str>,
    ) -> ApiResult<DefaultStripeCredentialsResponse> {
        let mut transaction = self.pool().begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(7420019001)")
            .execute(&mut *transaction)
            .await?;
        let row = sqlx::query(
            "SELECT * FROM billing_default_stripe_credentials WHERE singleton_id=1 FOR UPDATE",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        validate_default_version(row.as_ref(), expected_version)?;
        let vault = self.credential_vault()?;
        let api_ciphertext = match secret_key {
            Some(secret) => vault.seal(
                DEFAULT_SCOPE_ID,
                DEFAULT_SCOPE_ID,
                "stripe_default_api",
                secret,
            )?,
            None => row
                .as_ref()
                .map(|current| current.get("api_secret_reference"))
                .ok_or_else(missing_default_secret)?,
        };
        let webhook_ciphertext = match webhook_secret {
            Some(secret) => Some(vault.seal(
                DEFAULT_SCOPE_ID,
                DEFAULT_SCOPE_ID,
                "stripe_default_webhook",
                secret,
            )?),
            None => row
                .as_ref()
                .and_then(|current| current.try_get("webhook_secret_reference").ok())
                .flatten(),
        };
        let version = expected_version.checked_add(1).ok_or_else(|| {
            ApiError::unprocessable(
                "default_stripe_credentials_version_invalid",
                format!("expected_version {expected_version} cannot be incremented"),
            )
        })?;
        sqlx::query("INSERT INTO billing_default_stripe_credentials (singleton_id,environment,provider_account_reference,api_secret_reference,webhook_secret_reference,configuration_version) VALUES (1,$1,$2,$3,$4,$5) ON CONFLICT (singleton_id) DO UPDATE SET environment=EXCLUDED.environment,provider_account_reference=EXCLUDED.provider_account_reference,api_secret_reference=EXCLUDED.api_secret_reference,webhook_secret_reference=EXCLUDED.webhook_secret_reference,configuration_version=EXCLUDED.configuration_version,updated_at=now()")
            .bind(environment).bind(account_reference).bind(api_ciphertext).bind(webhook_ciphertext).bind(version)
            .execute(&mut *transaction).await?;
        transaction.commit().await?;
        self.default_stripe_credentials().await
    }

    pub async fn provision_workspace_default_stripe(
        &self,
        workspace_id: Uuid,
        defaults: &DefaultStripeSecrets,
    ) -> ApiResult<()> {
        let connection_id = Uuid::new_v4();
        let vault = self.credential_vault()?;
        let api_ciphertext = vault.seal(
            workspace_id,
            connection_id,
            "stripe_api",
            &defaults.api_secret,
        )?;
        let webhook_ciphertext = defaults
            .webhook_secret
            .as_deref()
            .map(|secret| vault.seal(workspace_id, connection_id, "stripe_webhook", secret))
            .transpose()?;
        let status = if webhook_ciphertext.is_some() {
            "ACTIVE"
        } else {
            "PENDING_SETUP"
        };
        let mut transaction = self.pool().begin().await?;
        let inserted = sqlx::query("INSERT INTO billing_connections (billing_connection_id,workspace_id,provider,external_account_reference,secret_reference,webhook_secret_reference,capabilities,status,environment,provider_account_reference) VALUES ($1,$2,'STRIPE',$3,$4,$5,ARRAY['CARD','SETUP_SESSION','OFF_SESSION','WEBHOOK'],$6,$7,$3) ON CONFLICT DO NOTHING")
            .bind(connection_id).bind(workspace_id).bind(&defaults.account_reference).bind(api_ciphertext)
            .bind(webhook_ciphertext).bind(status).bind(&defaults.environment)
            .execute(&mut *transaction).await?;
        if inserted.rows_affected() == 1 {
            audit_default_provision(&mut transaction, workspace_id, connection_id).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

async fn audit_default_provision(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: Uuid,
    connection_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_events (audit_event_id,workspace_id,actor_reference,action,resource_kind,resource_id,correlation_id,details) VALUES ($1,$2,'system-default-stripe','integration.default_provisioned','billing_connection',$3,$4,'{}'::jsonb)")
        .bind(Uuid::new_v4()).bind(workspace_id).bind(connection_id).bind(Uuid::new_v4())
        .execute(&mut **transaction).await?;
    Ok(())
}

fn validate_default_version(row: Option<&sqlx::postgres::PgRow>, expected: i32) -> ApiResult<()> {
    let actual = row
        .map(|row| row.get::<i32, _>("configuration_version"))
        .unwrap_or(0);
    if actual == expected {
        return Ok(());
    }
    Err(ApiError::conflict(
        "default_stripe_credentials_changed",
        format!("default Stripe credentials are at version {actual}, expected version {expected}"),
    ))
}

fn missing_default_secret() -> ApiError {
    ApiError::unprocessable(
        "default_stripe_secret_required",
        "secret_key is required before default Stripe credentials can be configured",
    )
}
