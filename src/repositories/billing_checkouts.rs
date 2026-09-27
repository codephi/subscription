use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::checkouts::{CheckoutKind, CheckoutResponse, CreateCheckoutRequest},
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

#[derive(Clone, Debug)]
pub enum CheckoutClaim {
    Claimed(CheckoutRecord),
    Busy(CheckoutRecord),
    Existing(CheckoutRecord),
}

#[derive(Clone, Debug)]
pub struct CheckoutRecord {
    pub checkout_id: Uuid,
    pub workspace_id: Uuid,
    pub customer_plan_id: Uuid,
    pub checkout_kind: CheckoutKind,
    pub on_demand_plan_id: Option<Uuid>,
    pub transaction_id: String,
    pub idempotency_key: String,
    pub request_sha256: String,
    pub collection_request_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl DatabaseRepository {
    pub async fn claim_checkout(
        &self,
        workspace_id: Uuid,
        key: &str,
        request: &CreateCheckoutRequest,
        request_hash: &str,
    ) -> ApiResult<CheckoutClaim> {
        let mut transaction = self.pool().begin().await?;
        lock_checkout_key(&mut transaction, workspace_id, key).await?;
        let record = match find_by_key(&mut transaction, workspace_id, key).await? {
            Some(record) => {
                validate_same_request(&record, request_hash)?;
                record
            }
            None => {
                insert_checkout(&mut transaction, workspace_id, key, request, request_hash).await?
            }
        };
        if record.collection_request_id.is_some() {
            transaction.commit().await?;
            return Ok(CheckoutClaim::Existing(record));
        }
        let claimed = take_lease(&mut transaction, record.checkout_id).await?;
        transaction.commit().await?;
        if claimed {
            Ok(CheckoutClaim::Claimed(record))
        } else {
            Ok(CheckoutClaim::Busy(record))
        }
    }

    pub async fn finish_checkout(
        &self,
        checkout_id: Uuid,
        collection_request_id: Uuid,
    ) -> ApiResult<()> {
        let updated = sqlx::query(
            "UPDATE billing_checkouts SET collection_request_id=$2,lease_expires_at=NULL \
             WHERE checkout_id=$1 AND collection_request_id IS NULL",
        )
        .bind(checkout_id)
        .bind(collection_request_id)
        .execute(&self.pool())
        .await?;
        if updated.rows_affected() == 1 {
            return Ok(());
        }
        let record = self.checkout(checkout_id, None).await?;
        if record.collection_request_id == Some(collection_request_id) {
            return Ok(());
        }
        Err(ApiError::conflict(
            "checkout_collection_mismatch",
            format!("checkout {checkout_id} is already linked to a different collection request"),
        ))
    }

    pub async fn release_checkout(&self, checkout_id: Uuid) -> ApiResult<()> {
        sqlx::query("UPDATE billing_checkouts SET lease_expires_at=NULL WHERE checkout_id=$1 AND collection_request_id IS NULL")
            .bind(checkout_id).execute(&self.pool()).await?;
        Ok(())
    }

    pub async fn checkout(
        &self,
        checkout_id: Uuid,
        workspace_id: Option<Uuid>,
    ) -> ApiResult<CheckoutRecord> {
        let row = sqlx::query(
            "SELECT * FROM billing_checkouts WHERE checkout_id=$1 AND ($2::uuid IS NULL OR workspace_id=$2)",
        )
        .bind(checkout_id)
        .bind(workspace_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| ApiError::not_found("checkout_not_found", format!("checkout {checkout_id} does not exist")))?;
        Ok(checkout_from_row(&row))
    }

    pub fn response_for_checkout(
        &self,
        record: CheckoutRecord,
        collection: Option<&crate::dto::billing::CollectionRequestResponse>,
    ) -> CheckoutResponse {
        let (status, amount_minor, currency, credits) = match collection {
            Some(item) => (
                checkout_status(&item.status).to_string(),
                Some(item.amount_minor),
                Some(item.currency.clone()),
                Some(item.granted_credit_units),
            ),
            None => ("PENDING".to_string(), None, None, None),
        };
        CheckoutResponse {
            checkout_id: record.checkout_id,
            customer_plan_id: record.customer_plan_id,
            checkout_kind: record.checkout_kind,
            status,
            collection_request_id: collection.map(|item| item.collection_request_id),
            amount_minor,
            currency,
            granted_credit_units: credits,
            transaction_id: record.transaction_id,
            created_at: record.created_at,
        }
    }
}

async fn lock_checkout_key(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("checkout:{workspace_id}:{key}"))
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn find_by_key(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    key: &str,
) -> Result<Option<CheckoutRecord>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT * FROM billing_checkouts WHERE workspace_id=$1 AND idempotency_key=$2 FOR UPDATE",
    )
    .bind(workspace_id)
    .bind(key)
    .fetch_optional(&mut **transaction)
    .await?
    .as_ref()
    .map(checkout_from_row))
}

async fn insert_checkout(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    key: &str,
    request: &CreateCheckoutRequest,
    request_hash: &str,
) -> ApiResult<CheckoutRecord> {
    let row = sqlx::query(
        "INSERT INTO billing_checkouts (checkout_id,workspace_id,customer_plan_id,checkout_kind, \
         on_demand_plan_id,transaction_id,idempotency_key,request_sha256) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(request.customer_plan_id)
    .bind(checkout_kind_text(request.checkout_kind))
    .bind(request.on_demand_plan_id)
    .bind(&request.transaction_id)
    .bind(key)
    .bind(request_hash)
    .fetch_one(&mut **transaction)
    .await
    .map_err(ApiError::from)?;
    Ok(checkout_from_row(&row))
}

async fn take_lease(
    transaction: &mut Transaction<'_, Postgres>,
    checkout_id: Uuid,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE billing_checkouts SET lease_expires_at=clock_timestamp()+interval '5 minutes' \
        WHERE checkout_id=$1 AND collection_request_id IS NULL \
        AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())",
    )
    .bind(checkout_id)
    .execute(&mut **transaction)
    .await?
    .rows_affected()
        == 1)
}

fn validate_same_request(record: &CheckoutRecord, request_hash: &str) -> ApiResult<()> {
    if record.request_sha256 == request_hash {
        return Ok(());
    }
    Err(ApiError::conflict(
        "checkout_idempotency_conflict",
        format!(
            "checkout key {:?} was already used with different terms",
            record.idempotency_key
        ),
    ))
}

fn checkout_from_row(row: &sqlx::postgres::PgRow) -> CheckoutRecord {
    CheckoutRecord {
        checkout_id: row.get("checkout_id"),
        workspace_id: row.get("workspace_id"),
        customer_plan_id: row.get("customer_plan_id"),
        checkout_kind: match row.get::<String, _>("checkout_kind").as_str() {
            "INITIAL" => CheckoutKind::Initial,
            _ => CheckoutKind::OnDemand,
        },
        on_demand_plan_id: row.get("on_demand_plan_id"),
        transaction_id: row.get("transaction_id"),
        idempotency_key: row.get("idempotency_key"),
        request_sha256: row.get("request_sha256"),
        collection_request_id: row.get("collection_request_id"),
        created_at: row.get("created_at"),
    }
}

fn checkout_kind_text(kind: CheckoutKind) -> &'static str {
    match kind {
        CheckoutKind::Initial => "INITIAL",
        CheckoutKind::OnDemand => "ON_DEMAND",
    }
}

fn checkout_status(status: &str) -> &'static str {
    match status {
        "PAID" => "PAID",
        "EXPIRED" => "EXPIRED",
        "EXHAUSTED" | "CANCELED" | "UNMATCHED" => "FAILED",
        _ => "PENDING",
    }
}
