use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::billing::AccountIntegrationResponse,
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

#[derive(Clone)]
pub struct IntegrationSecrets {
    pub account_id: Uuid,
    pub billing_connection_id: Uuid,
    pub provider: String,
    pub external_account_reference: String,
    pub provider_account_reference: Option<String>,
    pub provider_customer_reference: Option<String>,
    pub environment: Option<String>,
    pub status: String,
    pub secret_reference: String,
    pub webhook_secret_reference: Option<String>,
    pub configuration_version: i32,
}

pub struct CheckoutWebhookScope {
    pub account_id: Uuid,
    pub billing_connection_id: Uuid,
    pub provider: String,
    pub environment: Option<String>,
    pub provider_account_reference: Option<String>,
    pub provider_customer_reference: Option<String>,
}

impl DatabaseRepository {
    pub async fn checkout_webhook_scope(
        &self,
        collection_request_id: Uuid,
    ) -> ApiResult<CheckoutWebhookScope> {
        let row = sqlx::query("SELECT bc.account_id,bc.billing_connection_id,bc.provider,bc.environment, \
            bc.provider_account_reference,bc.provider_customer_reference FROM collection_requests cr \
            JOIN payment_method_bindings pmb ON pmb.payment_method_binding_id=cr.payment_method_binding_id \
            JOIN billing_connections bc ON bc.billing_connection_id=pmb.billing_connection_id \
            WHERE cr.collection_request_id=$1")
            .bind(collection_request_id).fetch_optional(&self.pool()).await?
            .ok_or_else(|| ApiError::not_found("collection_request_not_found", format!(
                "collection request {collection_request_id} does not exist")))?;
        Ok(CheckoutWebhookScope {
            account_id: row.get("account_id"),
            billing_connection_id: row.get("billing_connection_id"),
            provider: row.get("provider"),
            environment: row.try_get("environment").unwrap_or(None),
            provider_account_reference: row.try_get("provider_account_reference").unwrap_or(None),
            provider_customer_reference: row.try_get("provider_customer_reference").unwrap_or(None),
        })
    }

    pub async fn find_test_stripe_integration(
        &self,
        account_id: Uuid,
        stripe_account_id: &str,
    ) -> ApiResult<Option<AccountIntegrationResponse>> {
        let row = sqlx::query(
            "SELECT * FROM billing_connections WHERE account_id=$1 AND provider='STRIPE' \
             AND environment='TEST' AND provider_account_reference=$2",
        )
        .bind(account_id)
        .bind(stripe_account_id)
        .fetch_optional(&self.pool())
        .await?;
        Ok(row.as_ref().map(integration_from_row))
    }

    pub async fn get_integration(
        &self,
        account_id: Uuid,
        connection_id: Uuid,
    ) -> ApiResult<AccountIntegrationResponse> {
        let row = sqlx::query(
            "SELECT * FROM billing_connections WHERE account_id=$1 AND billing_connection_id=$2",
        )
        .bind(account_id)
        .bind(connection_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing_integration(account_id, connection_id))?;
        Ok(integration_from_row(&row))
    }

    pub async fn create_stripe_integration(
        &self,
        account_id: Uuid,
        stripe_account_id: &str,
        environment: &str,
        customer_id: Option<&str>,
        secret: &str,
    ) -> ApiResult<AccountIntegrationResponse> {
        let id = Uuid::new_v4();
        let ciphertext = self
            .credential_vault()?
            .seal(account_id, id, "stripe_api", secret)?;
        let row = sqlx::query(
            "INSERT INTO billing_connections (billing_connection_id,account_id,provider, \
             external_account_reference,secret_reference,capabilities,status,environment, \
             provider_account_reference,provider_customer_reference) \
             VALUES ($1,$2,'STRIPE',$3,$4,ARRAY['CARD','SETUP_SESSION','OFF_SESSION','WEBHOOK'], \
             'PENDING_SETUP',$5,$3,$6) RETURNING *",
        )
        .bind(id)
        .bind(account_id)
        .bind(stripe_account_id)
        .bind(ciphertext)
        .bind(environment)
        .bind(customer_id)
        .fetch_one(&self.pool())
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(database) if database.code().as_deref() == Some("23505") => {
                ApiError::conflict(
                    "integration_already_exists",
                    format!(
                        "Stripe account {stripe_account_id} is already connected for {environment}"
                    ),
                )
            }
            _ => ApiError::from(error),
        })?;
        audit_configuration(&self.pool(), account_id, id, "integration.created").await?;
        Ok(integration_from_row(&row))
    }

    pub async fn list_integrations(
        &self,
        account_id: Uuid,
    ) -> ApiResult<Vec<AccountIntegrationResponse>> {
        let rows = sqlx::query(
            "SELECT * FROM billing_connections WHERE account_id=$1 \
             ORDER BY created_at,billing_connection_id",
        )
        .bind(account_id)
        .fetch_all(&self.pool())
        .await?;
        Ok(rows.iter().map(integration_from_row).collect())
    }

    pub async fn integration_secrets(
        &self,
        account_id: Uuid,
        connection_id: Uuid,
    ) -> ApiResult<IntegrationSecrets> {
        let row = sqlx::query(
            "SELECT * FROM billing_connections WHERE account_id=$1 AND billing_connection_id=$2",
        )
        .bind(account_id)
        .bind(connection_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing_integration(account_id, connection_id))?;
        Ok(secrets_from_row(&row))
    }

    pub async fn update_stripe_integration(
        &self,
        account_id: Uuid,
        connection_id: Uuid,
        expected_version: i32,
        secret: Option<&str>,
        webhook_secret: Option<&str>,
    ) -> ApiResult<AccountIntegrationResponse> {
        let current = self.integration_secrets(account_id, connection_id).await?;
        let api_ciphertext = seal_optional(self, account_id, connection_id, "stripe_api", secret)?;
        let webhook_ciphertext = seal_optional(
            self,
            account_id,
            connection_id,
            "stripe_webhook",
            webhook_secret,
        )?;
        let row = update_integration_row(
            self,
            account_id,
            connection_id,
            expected_version,
            &current,
            api_ciphertext.as_deref(),
            webhook_ciphertext.as_deref(),
        )
        .await?;
        audit_configuration(
            &self.pool(),
            account_id,
            connection_id,
            "integration.updated",
        )
        .await?;
        Ok(integration_from_row(&row))
    }

    pub async fn activate_stripe_integration(
        &self,
        account_id: Uuid,
        connection_id: Uuid,
        expected_version: i32,
    ) -> ApiResult<AccountIntegrationResponse> {
        let row = sqlx::query(
            "UPDATE billing_connections SET status='ACTIVE',configuration_version=configuration_version+1 \
             WHERE account_id=$1 AND billing_connection_id=$2 AND configuration_version=$3 \
             AND status='PENDING_SETUP' AND webhook_secret_reference IS NOT NULL RETURNING *",
        )
        .bind(account_id)
        .bind(connection_id)
        .bind(expected_version)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| version_conflict(connection_id))?;
        audit_configuration(
            &self.pool(),
            account_id,
            connection_id,
            "integration.activated",
        )
        .await?;
        Ok(integration_from_row(&row))
    }

    pub async fn begin_customer_operation(
        &self,
        account_id: Uuid,
        connection_id: Uuid,
    ) -> ApiResult<Option<String>> {
        let inserted = sqlx::query(
            "INSERT INTO billing_integration_customer_operations \
             (billing_connection_id,account_id,status) VALUES ($1,$2,'STARTED') \
             ON CONFLICT (billing_connection_id) DO NOTHING RETURNING billing_connection_id",
        )
        .bind(connection_id)
        .bind(account_id)
        .fetch_optional(&self.pool())
        .await?;
        if inserted.is_some() {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT status,provider_customer_reference FROM billing_integration_customer_operations \
             WHERE billing_connection_id=$1 AND account_id=$2",
        )
        .bind(connection_id)
        .bind(account_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing_integration(account_id, connection_id))?;
        if row.get::<String, _>("status") == "COMPLETE" {
            return Ok(row.try_get("provider_customer_reference").unwrap_or(None));
        }
        Err(ApiError::conflict(
            "stripe_customer_setup_pending",
            format!("integration {connection_id} has an uncertain customer creation operation"),
        ))
    }

    pub async fn finish_customer_operation(
        &self,
        account_id: Uuid,
        connection_id: Uuid,
        customer_id: &str,
    ) -> ApiResult<()> {
        let mut transaction = self.pool().begin().await?;
        sqlx::query(
            "UPDATE billing_integration_customer_operations SET status='COMPLETE', \
             provider_customer_reference=$3 WHERE billing_connection_id=$1 AND account_id=$2",
        )
        .bind(connection_id)
        .bind(account_id)
        .bind(customer_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE billing_connections SET provider_customer_reference=$3 \
             WHERE account_id=$1 AND billing_connection_id=$2 AND provider_customer_reference IS NULL",
        )
        .bind(account_id)
        .bind(connection_id)
        .bind(customer_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn update_integration_row(
    repository: &DatabaseRepository,
    account_id: Uuid,
    connection_id: Uuid,
    expected_version: i32,
    current: &IntegrationSecrets,
    api_ciphertext: Option<&str>,
    webhook_ciphertext: Option<&str>,
) -> ApiResult<sqlx::postgres::PgRow> {
    let next_status = updated_status(current, webhook_ciphertext.is_some());
    sqlx::query(
        "UPDATE billing_connections SET secret_reference=COALESCE($4,secret_reference), \
         webhook_secret_reference=COALESCE($5,webhook_secret_reference), status=$6, \
         configuration_version=configuration_version+1 WHERE account_id=$1 \
         AND billing_connection_id=$2 AND configuration_version=$3 RETURNING *",
    )
    .bind(account_id)
    .bind(connection_id)
    .bind(expected_version)
    .bind(api_ciphertext)
    .bind(webhook_ciphertext)
    .bind(next_status)
    .fetch_optional(&repository.pool())
    .await?
    .ok_or_else(|| version_conflict(connection_id))
}

fn updated_status(current: &IntegrationSecrets, adds_webhook: bool) -> &'static str {
    if current.status == "ACTIVE" || current.webhook_secret_reference.is_some() || adds_webhook {
        "ACTIVE"
    } else {
        "PENDING_SETUP"
    }
}

fn seal_optional(
    repository: &DatabaseRepository,
    account_id: Uuid,
    connection_id: Uuid,
    purpose: &str,
    secret: Option<&str>,
) -> ApiResult<Option<String>> {
    secret
        .map(|secret| {
            repository
                .credential_vault()?
                .seal(account_id, connection_id, purpose, secret)
        })
        .transpose()
}

async fn audit_configuration(
    pool: &sqlx::PgPool,
    account_id: Uuid,
    connection_id: Uuid,
    action: &str,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO audit_events (audit_event_id,account_id,actor_reference,action, \
         resource_kind,resource_id,correlation_id,details) VALUES ($1,$2,'admin-ui',$3, \
         'billing_connection',$4,$5,'{}'::jsonb)",
    )
    .bind(Uuid::new_v4())
    .bind(account_id)
    .bind(action)
    .bind(connection_id)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await?;
    Ok(())
}

fn integration_from_row(row: &sqlx::postgres::PgRow) -> AccountIntegrationResponse {
    let connection_id: Uuid = row.get("billing_connection_id");
    AccountIntegrationResponse {
        billing_connection_id: connection_id,
        provider: row.get("provider"),
        account_reference: row
            .try_get("provider_account_reference")
            .unwrap_or_else(|_| row.get("external_account_reference")),
        environment: row
            .try_get::<Option<String>, _>("environment")
            .ok()
            .flatten()
            .unwrap_or_else(|| "LEGACY".into()),
        status: row.get("status"),
        api_secret_configured: true,
        webhook_secret_configured: row
            .try_get::<Option<String>, _>("webhook_secret_reference")
            .ok()
            .flatten()
            .is_some(),
        customer_reference: row.try_get("provider_customer_reference").unwrap_or(None),
        webhook_path: format!("/v1/billing/webhooks/{connection_id}"),
        webhook_url: None,
        configuration_version: row.try_get("configuration_version").unwrap_or(1),
    }
}

fn secrets_from_row(row: &sqlx::postgres::PgRow) -> IntegrationSecrets {
    IntegrationSecrets {
        account_id: row.get("account_id"),
        billing_connection_id: row.get("billing_connection_id"),
        provider: row.get("provider"),
        external_account_reference: row.get("external_account_reference"),
        provider_account_reference: row.try_get("provider_account_reference").unwrap_or(None),
        provider_customer_reference: row.try_get("provider_customer_reference").unwrap_or(None),
        environment: row.try_get("environment").unwrap_or(None),
        status: row.get("status"),
        secret_reference: row.get("secret_reference"),
        webhook_secret_reference: row.try_get("webhook_secret_reference").unwrap_or(None),
        configuration_version: row.try_get("configuration_version").unwrap_or(1),
    }
}

fn version_conflict(connection_id: Uuid) -> ApiError {
    ApiError::conflict(
        "integration_version_conflict",
        format!("integration {connection_id} changed or is not ready"),
    )
}

fn missing_integration(account_id: Uuid, connection_id: Uuid) -> ApiError {
    ApiError::not_found(
        "integration_not_found",
        format!("integration {connection_id} does not exist in account {account_id}"),
    )
}
