use chrono::Utc;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::admin_queries::{
        AccountPageResponse, AccountProjectionResponse, CreateAccountRequest,
        CustomerPlanPageResponse,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
    /// Create an administratively initialized account; e.g. `repo.create_account(&request).await`.
    pub async fn create_account(
        &self,
        request: &CreateAccountRequest,
    ) -> ApiResult<AccountProjectionResponse> {
        let account = NewAdminAccount::new();
        let mut transaction = self.pool().begin().await?;
        insert_admin_account(&mut transaction, &account, &request.actor_reference).await?;
        transaction.commit().await?;
        self.find_account_projection(account.account_id).await
    }

    /// Page account projections by stable ID; e.g. `repo.list_account_projections(None, 20).await`.
    pub async fn list_account_projections(
        &self,
        cursor: Option<Uuid>,
        limit: i64,
    ) -> ApiResult<AccountPageResponse> {
        let rows = sqlx::query("SELECT * FROM account_projections WHERE ($1::uuid IS NULL OR account_id > $1) ORDER BY account_id LIMIT $2")
            .bind(cursor).bind(limit + 1).fetch_all(&self.pool()).await?;
        let has_more = rows.len() as i64 > limit;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(account_from_row)
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| items.last().map(|item| item.account_id))
            .flatten();
        Ok(AccountPageResponse { items, next_cursor })
    }

    /// Read a account projection; e.g. `repo.find_account_projection(id).await`.
    pub async fn find_account_projection(&self, id: Uuid) -> ApiResult<AccountProjectionResponse> {
        let row = sqlx::query("SELECT * FROM account_projections WHERE account_id=$1")
            .bind(id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| {
                ApiError::not_found("account_not_found", format!("account {id} does not exist"))
            })?;
        Ok(account_from_row(&row))
    }

    /// Page customer plans by stable ID; e.g. `repo.list_account_customer_plans(id, None, 20).await`.
    pub async fn list_account_customer_plans(
        &self,
        account_id: Uuid,
        cursor: Option<Uuid>,
        limit: i64,
    ) -> ApiResult<CustomerPlanPageResponse> {
        let ids = sqlx::query_scalar::<_, Uuid>("SELECT customer_plan_id FROM customer_plans WHERE customer_id=$1 AND ($2::uuid IS NULL OR customer_plan_id > $2) ORDER BY customer_plan_id LIMIT $3")
            .bind(account_id).bind(cursor).bind(limit + 1).fetch_all(&self.pool()).await?;
        let has_more = ids.len() as i64 > limit;
        let mut items = Vec::with_capacity(ids.len().min(limit as usize));
        for id in ids.iter().take(limit as usize) {
            items.push(self.find_customer_plan(account_id, *id).await?);
        }
        let next_cursor = has_more.then(|| ids[limit as usize - 1]);
        Ok(CustomerPlanPageResponse { items, next_cursor })
    }
}

struct NewAdminAccount {
    account_id: Uuid,
    event_id: Uuid,
    outbox_event_id: Uuid,
    correlation_id: Uuid,
    occurred_at: chrono::DateTime<Utc>,
    envelope: serde_json::Value,
    domain_event: serde_json::Value,
}

impl NewAdminAccount {
    fn new() -> Self {
        let account_id = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let outbox_event_id = Uuid::new_v4();
        let correlation_id = Uuid::new_v4();
        let occurred_at = Utc::now();
        Self {
            account_id,
            event_id,
            outbox_event_id,
            correlation_id,
            occurred_at,
            envelope: created_envelope(account_id, event_id, correlation_id, occurred_at),
            domain_event: projection_event(
                account_id,
                event_id,
                outbox_event_id,
                correlation_id,
                occurred_at,
            ),
        }
    }
}

fn created_envelope(
    account_id: Uuid,
    event_id: Uuid,
    correlation_id: Uuid,
    occurred_at: chrono::DateTime<Utc>,
) -> serde_json::Value {
    json!({"event_id":event_id,"event_type":"account.created","schema_version":1,
        "aggregate_id":account_id,"sequence":1,"occurred_at":occurred_at,
        "account_id":account_id,"correlation_id":correlation_id,"causation_id":null,
        "payload":{"account_id":account_id}})
}

fn projection_event(
    account_id: Uuid,
    event_id: Uuid,
    outbox_event_id: Uuid,
    correlation_id: Uuid,
    occurred_at: chrono::DateTime<Utc>,
) -> serde_json::Value {
    json!({"event_id":outbox_event_id,"event_type":"account.projection_updated",
        "schema_version":1,"aggregate_type":"account","aggregate_id":account_id,
        "sequence":1,"occurred_at":occurred_at,"account_id":account_id,
        "correlation_id":correlation_id,"causation_id":event_id,
        "payload":{"operational_status":"CREATED"}})
}

async fn insert_admin_account(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account: &NewAdminAccount,
    actor_reference: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO account_projections (account_id, operational_status, external_sequence, external_occurred_at, last_event_id) VALUES ($1,'CREATED',1,$2,$3)")
        .bind(account.account_id).bind(account.occurred_at).bind(account.event_id).execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO integration_inbox (event_id,account_id,event_type,schema_version,aggregate_id,external_sequence,occurred_at,correlation_id,payload,processing_status,processed_at) VALUES ($1,$2,'account.created',1,$2,1,$3,$4,$5,'PROCESSED',now())")
        .bind(account.event_id).bind(account.account_id).bind(account.occurred_at).bind(account.correlation_id).bind(&account.envelope).execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence,account_id,correlation_id,causation_id,payload,occurred_at) VALUES ($1,'account.projection_updated','account',$2,1,$2,$3,$4,$5,$6)")
        .bind(account.outbox_event_id).bind(account.account_id).bind(account.correlation_id).bind(account.event_id).bind(&account.domain_event).bind(account.occurred_at).execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO audit_events (audit_event_id,account_id,actor_reference,action,resource_kind,resource_id,correlation_id,details) VALUES ($1,$2,$3,'account.admin_created','account_projection',$2,$4,$5)")
        .bind(Uuid::new_v4()).bind(account.account_id).bind(actor_reference).bind(account.correlation_id).bind(json!({"operational_status":"CREATED","sequence":1})).execute(&mut **transaction).await?;
    Ok(())
}

fn account_from_row(row: &sqlx::postgres::PgRow) -> AccountProjectionResponse {
    AccountProjectionResponse {
        account_id: row.get("account_id"),
        operational_status: row.get("operational_status"),
        external_sequence: row.get("external_sequence"),
        external_occurred_at: row.get("external_occurred_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}
