use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::billing::{
        BillingConnectionResponse, CreateBillingConnectionRequest, PaymentMethodBindingResponse,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

pub struct BillingConnectorConfiguration {
    pub workspace_id: Uuid,
    pub billing_connection_id: Uuid,
    pub provider: String,
    pub external_account_reference: String,
    pub secret_reference: String,
    pub webhook_secret_reference: String,
    pub status: String,
    pub provider_account_reference: Option<String>,
    pub provider_customer_reference: Option<String>,
    pub environment: Option<String>,
    pub configuration_version: i32,
    pub managed: bool,
}

impl DatabaseRepository {
    pub async fn find_payment_method_binding_by_id(
        &self,
        workspace_id: Uuid,
        binding_id: Uuid,
    ) -> ApiResult<PaymentMethodBindingResponse> {
        let row = sqlx::query("SELECT * FROM payment_method_bindings WHERE workspace_id=$1 AND payment_method_binding_id=$2")
            .bind(workspace_id).bind(binding_id).fetch_optional(&self.pool()).await?
            .ok_or_else(|| ApiError::not_found("payment_method_binding_not_found", format!("payment method binding {binding_id} does not belong to workspace {workspace_id}")))?;
        let binding = binding_from_row(&row);
        if binding.status == "ACTIVE" {
            return Ok(binding);
        }
        Err(ApiError::conflict(
            "payment_method_binding_inactive",
            format!("payment method binding {binding_id} is {}", binding.status),
        ))
    }

    pub async fn find_payment_method_binding(
        &self,
        workspace_id: Uuid,
        connection_id: Uuid,
        provider_reference: &str,
    ) -> ApiResult<Option<PaymentMethodBindingResponse>> {
        let row = sqlx::query(
            "SELECT * FROM payment_method_bindings WHERE workspace_id=$1 \
            AND billing_connection_id=$2 AND provider_payment_method_reference=$3",
        )
        .bind(workspace_id)
        .bind(connection_id)
        .bind(provider_reference)
        .fetch_optional(&self.pool())
        .await?;
        Ok(row.as_ref().map(binding_from_row))
    }

    pub async fn billing_connector_configuration(
        &self,
        connection_id: Uuid,
    ) -> ApiResult<BillingConnectorConfiguration> {
        let row = sqlx::query(
            "SELECT * \
             FROM billing_connections WHERE billing_connection_id=$1",
        )
        .bind(connection_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| {
            ApiError::not_found(
                "billing_connection_not_found",
                format!("billing connection {connection_id} does not exist"),
            )
        })?;
        let secret_reference: String = row.get("secret_reference");
        let managed = secret_reference.starts_with("v1:");
        let webhook_secret_reference = row
            .get::<Option<String>, _>("webhook_secret_reference")
            .unwrap_or_else(|| {
                if managed {
                    String::new()
                } else {
                    secret_reference.clone()
                }
            });
        Ok(BillingConnectorConfiguration {
            workspace_id: row.get("workspace_id"),
            billing_connection_id: connection_id,
            provider: row.get("provider"),
            external_account_reference: row.get("external_account_reference"),
            secret_reference,
            webhook_secret_reference,
            status: row.get("status"),
            provider_account_reference: row.try_get("provider_account_reference").unwrap_or(None),
            provider_customer_reference: row.try_get("provider_customer_reference").unwrap_or(None),
            environment: row.try_get("environment").unwrap_or(None),
            configuration_version: row.try_get("configuration_version").unwrap_or(1),
            managed,
        })
    }

    pub async fn create_billing_connection(
        &self,
        workspace_id: Uuid,
        request: &CreateBillingConnectionRequest,
    ) -> ApiResult<BillingConnectionResponse> {
        ensure_active_workspace(&self.pool(), workspace_id).await?;
        let row = sqlx::query(
            "INSERT INTO billing_connections (billing_connection_id,workspace_id,provider, \
             external_account_reference,secret_reference,webhook_secret_reference,capabilities,status) \
             VALUES ($1,$2,$3,$4,$5,$6,ARRAY['CARD','SETUP_SESSION','OFF_SESSION','WEBHOOK'],'ACTIVE') \
             RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(&request.provider)
        .bind(&request.external_account_reference)
        .bind(&request.secret_reference)
        .bind(&request.webhook_secret_reference)
        .fetch_one(&self.pool())
        .await?;
        Ok(connection_from_row(&row))
    }

    pub async fn find_billing_connection(
        &self,
        workspace_id: Uuid,
        connection_id: Uuid,
    ) -> ApiResult<BillingConnectionResponse> {
        let row = sqlx::query(
            "SELECT * FROM billing_connections WHERE workspace_id=$1 AND billing_connection_id=$2",
        )
        .bind(workspace_id)
        .bind(connection_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing_connection(workspace_id, connection_id))?;
        Ok(connection_from_row(&row))
    }

    pub async fn ensure_customer_plan_workspace(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
    ) -> ApiResult<()> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM customer_plans WHERE customer_plan_id=$1 AND customer_id=$2)",
        )
        .bind(customer_plan_id)
        .bind(workspace_id)
        .fetch_one(&self.pool())
        .await?;
        if exists {
            return Ok(());
        }
        Err(ApiError::not_found(
            "customer_plan_not_found",
            format!("customer plan {customer_plan_id} does not belong to workspace {workspace_id}"),
        ))
    }

    pub async fn create_verified_payment_method_binding(
        &self,
        workspace_id: Uuid,
        connection_id: Uuid,
        customer_plan_id: Option<Uuid>,
        provider_payment_method_reference: &str,
    ) -> ApiResult<PaymentMethodBindingResponse> {
        validate_binding_reference(provider_payment_method_reference)?;
        if let Some(existing) = self
            .find_payment_method_binding(
                workspace_id,
                connection_id,
                provider_payment_method_reference,
            )
            .await?
        {
            return active_binding(existing);
        }
        let row = sqlx::query(
            "INSERT INTO payment_method_bindings (payment_method_binding_id,billing_connection_id, \
             workspace_id,customer_id,customer_plan_id,payment_method,provider_payment_method_reference,status) \
             SELECT $1,bc.billing_connection_id,$2,$2,$3,'CARD',$4,'ACTIVE' FROM billing_connections bc \
             WHERE bc.billing_connection_id=$5 AND bc.workspace_id=$2 AND bc.status='ACTIVE' \
             AND ($3::uuid IS NULL OR EXISTS (SELECT 1 FROM customer_plans cp \
               WHERE cp.customer_plan_id=$3 AND cp.customer_id=$2)) \
             ON CONFLICT (billing_connection_id,provider_payment_method_reference) DO NOTHING RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(customer_plan_id)
        .bind(provider_payment_method_reference)
        .bind(connection_id)
        .fetch_optional(&self.pool())
        .await?;
        if let Some(row) = row {
            return Ok(binding_from_row(&row));
        }
        if let Some(existing) = self
            .find_payment_method_binding(
                workspace_id,
                connection_id,
                provider_payment_method_reference,
            )
            .await?
        {
            return active_binding(existing);
        }
        Err(missing_connection(workspace_id, connection_id))
    }

    pub async fn list_payment_method_bindings(
        &self,
        workspace_id: Uuid,
    ) -> ApiResult<Vec<PaymentMethodBindingResponse>> {
        let rows = sqlx::query(
            "SELECT * FROM payment_method_bindings WHERE workspace_id=$1 ORDER BY created_at,payment_method_binding_id",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool())
        .await?;
        Ok(rows.iter().map(binding_from_row).collect())
    }
}

fn active_binding(
    binding: PaymentMethodBindingResponse,
) -> ApiResult<PaymentMethodBindingResponse> {
    if binding.status == "ACTIVE" {
        return Ok(binding);
    }
    Err(ApiError::conflict(
        "payment_method_binding_inactive",
        format!(
            "payment method binding {} is {}",
            binding.payment_method_binding_id, binding.status
        ),
    ))
}

async fn ensure_active_workspace(pool: &sqlx::PgPool, workspace_id: Uuid) -> ApiResult<()> {
    let status: Option<String> = sqlx::query_scalar(
        "SELECT operational_status FROM workspace_projections WHERE workspace_id=$1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    if status.as_deref() == Some("ACTIVE") {
        return Ok(());
    }
    Err(ApiError::conflict(
        "workspace_not_operational",
        format!("workspace {workspace_id} must exist with ACTIVE status"),
    ))
}

fn validate_binding_reference(reference: &str) -> ApiResult<()> {
    if reference.starts_with("pm_") && reference.len() <= 255 {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_payment_method_reference",
        format!("payment method reference {reference:?} must be a token beginning with pm_"),
    ))
}

fn connection_from_row(row: &sqlx::postgres::PgRow) -> BillingConnectionResponse {
    let id: Uuid = row.get("billing_connection_id");
    BillingConnectionResponse {
        billing_connection_id: id,
        workspace_id: row.get("workspace_id"),
        provider: row.get("provider"),
        external_account_reference: row.get("external_account_reference"),
        capabilities: row.get("capabilities"),
        status: row.get("status"),
        webhook_path: format!("/v1/billing/webhooks/{id}"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn binding_from_row(row: &sqlx::postgres::PgRow) -> PaymentMethodBindingResponse {
    PaymentMethodBindingResponse {
        payment_method_binding_id: row.get("payment_method_binding_id"),
        billing_connection_id: row.get("billing_connection_id"),
        workspace_id: row.get("workspace_id"),
        customer_plan_id: row.get("customer_plan_id"),
        payment_method: row.get("payment_method"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    }
}

fn missing_connection(workspace_id: Uuid, connection_id: Uuid) -> ApiError {
    ApiError::not_found(
        "billing_connection_not_found",
        format!("billing connection {connection_id} does not exist in workspace {workspace_id}"),
    )
}
