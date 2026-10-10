use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{
        promotions::{
            PromotionHistoryEntry, PromotionHistoryResponse, PromotionListQuery,
            PromotionPageResponse, PromotionResponse, RedeemVoucherRequest,
            VoucherRedemptionResponse,
        },
        units::CreditUnits,
    },
    error::{ApiError, ApiResult},
    repositories::{
        credit_rows::{entry_from_row, load_references},
        credits::{lock_active_customer_wallet, reserve_idempotency, reserve_transaction},
        database::DatabaseRepository,
    },
};

impl DatabaseRepository {
    pub async fn create_promotion(
        &self,
        kind: &str,
        body: Value,
        actor: Option<&str>,
    ) -> ApiResult<PromotionResponse> {
        let mut transaction = self.pool().begin().await?;
        let id = Uuid::new_v4();
        let code: String = body["code"].as_str().unwrap_or_default().to_string();
        let name: String = body["name"].as_str().unwrap_or_default().to_string();
        let description = body["description"].as_str();
        let valid_from = body["valid_from"].as_str().map(parse_time).transpose()?;
        let valid_until = body["valid_until"].as_str().map(parse_time).transpose()?;
        let max_total = body["max_total_uses"].as_i64();
        let max_account = body["max_uses_per_account"].as_i64();
        match kind {
            "VOUCHER" => {
                sqlx::query("INSERT INTO vouchers (voucher_id,code,name,description,credit_units,valid_from,valid_until,max_total_uses,max_uses_per_account) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                    .bind(id).bind(&code).bind(&name).bind(description).bind(json_i64(&body["credit_units"]))
                    .bind(valid_from).bind(valid_until).bind(max_total).bind(max_account)
                    .execute(&mut *transaction).await?;
            }
            "COUPON" => {
                sqlx::query("INSERT INTO coupons (coupon_id,code,name,description,discount_kind,discount_value,currency,applies_to_initial,applies_to_on_demand,valid_from,valid_until,max_total_uses,max_uses_per_account) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
                    .bind(id).bind(&code).bind(&name).bind(description).bind(body["discount_kind"].as_str())
                    .bind(body["discount_value"].as_i64()).bind(body["currency"].as_str())
                    .bind(body["applies_to_initial"].as_bool().unwrap_or(false))
                    .bind(body["applies_to_on_demand"].as_bool().unwrap_or(false))
                    .bind(valid_from).bind(valid_until).bind(max_total).bind(max_account)
                    .execute(&mut *transaction).await?;
            }
            _ => return Err(invalid_kind(kind)),
        }
        insert_history(
            &mut transaction,
            PromotionHistorySnapshot {
                kind,
                id,
                version: 1,
                action: "CREATED",
                actor,
                before: json!({}),
                after: body,
            },
        )
        .await?;
        transaction.commit().await?;
        self.get_promotion(kind, id).await
    }

    pub async fn list_promotions(
        &self,
        kind: &str,
        query: &PromotionListQuery,
        limit: i64,
    ) -> ApiResult<PromotionPageResponse> {
        let sql = promotion_select(kind)?;
        let statement = format!(
            "{sql} WHERE ($1::uuid IS NULL OR p.promotion_id<$1) AND ($2::text IS NULL OR p.status=$2) AND ($3::text IS NULL OR p.code ILIKE '%' || $3 || '%' OR p.name ILIKE '%' || $3 || '%') ORDER BY p.promotion_id DESC LIMIT $4"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(statement))
            .bind(query.cursor)
            .bind(&query.status)
            .bind(&query.search)
            .bind(limit + 1)
            .fetch_all(&self.pool())
            .await?;
        let has_more = rows.len() as i64 > limit;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(promotion_from_row)
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| items.last().map(|item| item.promotion_id))
            .flatten();
        Ok(PromotionPageResponse { items, next_cursor })
    }

    pub async fn get_promotion(&self, kind: &str, id: Uuid) -> ApiResult<PromotionResponse> {
        let statement = format!("{} WHERE p.promotion_id=$1", promotion_select(kind)?);
        let row = sqlx::query(sqlx::AssertSqlSafe(statement))
            .bind(id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| {
                ApiError::not_found("promotion_not_found", format!("{kind} {id} does not exist"))
            })?;
        Ok(promotion_from_row(&row))
    }

    pub async fn update_promotion(
        &self,
        kind: &str,
        id: Uuid,
        body: Value,
        actor: Option<&str>,
    ) -> ApiResult<PromotionResponse> {
        let mut transaction = self.pool().begin().await?;
        let before = load_promotion_json(&mut transaction, kind, id, true).await?;
        let expected = body["expected_version"].as_i64().unwrap_or_default();
        let current = before["version"].as_i64().unwrap_or_default();
        if expected != current {
            return Err(ApiError::conflict(
                "promotion_version_conflict",
                format!("{kind} {id} has version {current}, expected {expected}"),
            ));
        }
        let status = body["status"]
            .as_str()
            .unwrap_or(before["status"].as_str().unwrap_or("ACTIVE"));
        let valid_from = body
            .get("valid_from")
            .filter(|v| !v.is_null())
            .and_then(Value::as_str)
            .map(parse_time)
            .transpose()?;
        let valid_until = body
            .get("valid_until")
            .filter(|v| !v.is_null())
            .and_then(Value::as_str)
            .map(parse_time)
            .transpose()?;
        let max_total = body
            .get("max_total_uses")
            .filter(|v| !v.is_null())
            .and_then(Value::as_i64);
        let max_account = body
            .get("max_uses_per_account")
            .filter(|v| !v.is_null())
            .and_then(Value::as_i64);
        validate_editable_status(status, &before)?;
        validate_usage_limits(&mut transaction, kind, id, max_total, max_account).await?;
        let table = table_for(kind)?;
        let statement = format!(
            "UPDATE {table} SET status=$2,valid_from=$3,valid_until=$4,max_total_uses=$5,max_uses_per_account=$6,version=version+1 WHERE {}=$1 AND version=$7 RETURNING version",
            id_column(kind)?
        );
        let version = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(statement))
            .bind(id)
            .bind(status)
            .bind(valid_from)
            .bind(valid_until)
            .bind(max_total)
            .bind(max_account)
            .bind(expected)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| {
                ApiError::conflict(
                    "promotion_version_conflict",
                    format!("{kind} {id} changed during update"),
                )
            })?;
        let after = load_promotion_json(&mut transaction, kind, id, false).await?;
        insert_history(
            &mut transaction,
            PromotionHistorySnapshot {
                kind,
                id,
                version,
                action: "UPDATED",
                actor,
                before,
                after,
            },
        )
        .await?;
        transaction.commit().await?;
        self.get_promotion(kind, id).await
    }

    pub async fn promotion_history(
        &self,
        kind: &str,
        id: Uuid,
    ) -> ApiResult<PromotionHistoryResponse> {
        self.get_promotion(kind, id).await?;
        let rows = sqlx::query("SELECT version,action,actor_reference,before_snapshot,after_snapshot,occurred_at FROM promotion_history WHERE promotion_kind=$1 AND promotion_id=$2 ORDER BY version DESC")
            .bind(kind).bind(id).fetch_all(&self.pool()).await?;
        Ok(PromotionHistoryResponse {
            items: rows
                .iter()
                .map(|row| PromotionHistoryEntry {
                    version: row.get("version"),
                    action: row.get("action"),
                    actor_reference: row.get("actor_reference"),
                    before_snapshot: row.get("before_snapshot"),
                    after_snapshot: row.get("after_snapshot"),
                    occurred_at: row.get("occurred_at"),
                })
                .collect(),
        })
    }

    pub async fn redeem_voucher(
        &self,
        account_id: Uuid,
        key: &str,
        hash: &str,
        request: &RedeemVoucherRequest,
    ) -> ApiResult<VoucherRedemptionResponse> {
        let mut transaction = self.pool().begin().await?;
        let wallet = lock_active_customer_wallet(&mut transaction, account_id).await?;
        reserve_idempotency(
            &mut transaction,
            account_id,
            key,
            hash,
            "VOUCHER_REDEMPTION",
        )
        .await?;
        reserve_transaction(
            &mut transaction,
            account_id,
            &request.transaction_id,
            "VOUCHER_REDEMPTION",
        )
        .await?;
        let voucher = sqlx::query("SELECT * FROM vouchers WHERE ($1::uuid IS NOT NULL AND voucher_id=$1) OR ($2::text IS NOT NULL AND code=$2) FOR UPDATE")
            .bind(request.voucher_id).bind(request.code.as_deref()).fetch_optional(&mut *transaction).await?
            .ok_or_else(|| ApiError::not_found("voucher_not_found", "voucher identifier does not resolve to a voucher"))?;
        let voucher_id: Uuid = voucher.get("voucher_id");
        let units = voucher.get::<i64, _>("credit_units");
        validate_active_window(&voucher, "voucher", voucher_id)?;
        enforce_limits(
            &mut transaction,
            "VOUCHER",
            voucher_id,
            account_id,
            &voucher,
        )
        .await?;
        let new_balance = wallet.balance.checked_add(CreditUnits::new(units))?;
        let entry_id = Uuid::new_v4();
        let lot_id = Uuid::new_v4();
        let redemption_id = Uuid::new_v4();
        let entry = sqlx::query("INSERT INTO customer_wallet_entries (customer_wallet_entry_id,customer_wallet_id,customer_id,entry_sequence,entry_type,source_channel,signed_credit_units,balance_before_credit_units,balance_after_credit_units,transaction_id,description,metadata,request_id) VALUES ($1,$2,$3,$4,'VOUCHER_CREDIT','voucher',$5,$6,$7,$8,$9,$10,$11) RETURNING *")
            .bind(entry_id).bind(wallet.wallet_id).bind(account_id).bind(wallet.next_sequence).bind(units)
            .bind(wallet.balance.value()).bind(new_balance.value()).bind(&request.transaction_id)
            .bind(request.description.as_deref()).bind(json!({"voucher_id":voucher_id})).bind(Uuid::new_v4())
            .fetch_one(&mut *transaction).await?;
        sqlx::query("INSERT INTO credit_lots (credit_lot_id,customer_id,granting_entry_id,source_kind,original_credit_units,remaining_credit_units) VALUES ($1,$2,$3,'VOUCHER',$4,$4)")
            .bind(lot_id).bind(account_id).bind(entry_id).bind(units).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO voucher_redemptions (voucher_redemption_id,voucher_id,account_id,transaction_id,customer_wallet_entry_id,credit_lot_id,credit_units) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(redemption_id).bind(voucher_id).bind(account_id).bind(&request.transaction_id).bind(entry_id).bind(lot_id).bind(units)
            .execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO wallet_transaction_references (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind,voucher_id) VALUES ($1,$2,'VOUCHER',$3)")
            .bind(Uuid::new_v4()).bind(entry_id).bind(voucher_id).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO wallet_transaction_references (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind,credit_lot_id) VALUES ($1,$2,'CREDIT_LOT',$3)")
            .bind(Uuid::new_v4()).bind(entry_id).bind(lot_id).execute(&mut *transaction).await?;
        increment_usage(&mut transaction, "VOUCHER", voucher_id, account_id).await?;
        super::credit_writes::update_wallet_balance(
            &mut transaction,
            wallet.wallet_id,
            new_balance,
        )
        .await?;
        super::credit_writes::insert_credit_outbox(
            &mut transaction,
            account_id,
            wallet.wallet_id,
            wallet.next_sequence,
            entry_id,
            CreditUnits::new(units),
        )
        .await?;
        super::credit_writes::complete_reservations(
            &mut transaction,
            account_id,
            key,
            &request.transaction_id,
            redemption_id,
        )
        .await?;
        sqlx::query("INSERT INTO audit_events (audit_event_id,account_id,action,resource_kind,resource_id,correlation_id,details) VALUES ($1,$2,'voucher.redeemed','voucher',$3,$4,$5)")
            .bind(Uuid::new_v4()).bind(account_id).bind(voucher_id).bind(Uuid::new_v4())
            .bind(json!({"voucher_redemption_id":redemption_id,"customer_wallet_entry_id":entry_id,"transaction_id":request.transaction_id}))
            .execute(&mut *transaction).await?;
        transaction.commit().await?;
        let references = load_references(&self.pool(), entry_id).await?;
        let created_at: DateTime<Utc> = entry.get("created_at");
        Ok(VoucherRedemptionResponse {
            voucher_redemption_id: redemption_id,
            voucher_id,
            account_id,
            credit_units: CreditUnits::new(units),
            entry: entry_from_row(&entry, references),
            created_at,
        })
    }
}

async fn enforce_limits(
    transaction: &mut Transaction<'_, Postgres>,
    kind: &str,
    id: Uuid,
    account_id: Uuid,
    promotion: &sqlx::postgres::PgRow,
) -> ApiResult<()> {
    let (total_completed, total_reserved, account_completed, account_reserved) =
        usage_totals(transaction, kind, id, account_id).await?;
    check_limit(
        "total",
        promotion.get("max_total_uses"),
        total_completed,
        total_reserved,
    )?;
    check_limit(
        "account",
        promotion.get("max_uses_per_account"),
        account_completed,
        account_reserved,
    )
}

async fn usage_totals(
    transaction: &mut Transaction<'_, Postgres>,
    kind: &str,
    id: Uuid,
    account: Uuid,
) -> ApiResult<(i64, i64, i64, i64)> {
    if kind == "VOUCHER" {
        let all: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM voucher_redemptions WHERE voucher_id=$1",
        )
        .bind(id)
        .fetch_one(&mut **transaction)
        .await?;
        let local: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM voucher_redemptions WHERE voucher_id=$1 AND account_id=$2").bind(id).bind(account).fetch_one(&mut **transaction).await?;
        Ok((all, 0, local, 0))
    } else {
        let total = sqlx::query("SELECT count(*) FILTER(WHERE status='COMPLETED')::bigint completed,count(*) FILTER(WHERE status='RESERVED')::bigint reserved FROM coupon_checkout_reservations WHERE coupon_id=$1").bind(id).fetch_one(&mut **transaction).await?;
        let local = sqlx::query("SELECT count(*) FILTER(WHERE status='COMPLETED')::bigint completed,count(*) FILTER(WHERE status='RESERVED')::bigint reserved FROM coupon_checkout_reservations WHERE coupon_id=$1 AND account_id=$2").bind(id).bind(account).fetch_one(&mut **transaction).await?;
        Ok((
            total.get("completed"),
            total.get("reserved"),
            local.get("completed"),
            local.get("reserved"),
        ))
    }
}

fn check_limit(scope: &str, limit: Option<i64>, completed: i64, reserved: i64) -> ApiResult<()> {
    if limit.is_some_and(|max| completed + reserved >= max) {
        return Err(ApiError::conflict(
            "promotion_usage_limit_reached",
            format!("promotion {scope} usage limit has been reached"),
        ));
    }
    Ok(())
}

async fn increment_usage(
    transaction: &mut Transaction<'_, Postgres>,
    kind: &str,
    id: Uuid,
    account: Uuid,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO promotion_usage_counters (promotion_kind,promotion_id,account_id,completed_uses) VALUES ($1,$2,$3,1) ON CONFLICT (promotion_kind,promotion_id,account_id) DO UPDATE SET completed_uses=promotion_usage_counters.completed_uses+1")
        .bind(kind).bind(id).bind(account).execute(&mut **transaction).await?;
    Ok(())
}

fn validate_active_window(row: &sqlx::postgres::PgRow, kind: &str, id: Uuid) -> ApiResult<()> {
    let status: String = row.get("status");
    let now = Utc::now();
    let starts = row
        .get::<Option<DateTime<Utc>>, _>("valid_from")
        .is_none_or(|date| date <= now);
    let unexpired = row
        .get::<Option<DateTime<Utc>>, _>("valid_until")
        .is_none_or(|date| date > now);
    if status == "ACTIVE" && starts && unexpired {
        return Ok(());
    }
    Err(ApiError::conflict(
        "promotion_unavailable",
        format!("{kind} {id} is {status} or outside its validity window"),
    ))
}

async fn validate_usage_limits(
    transaction: &mut Transaction<'_, Postgres>,
    kind: &str,
    id: Uuid,
    total: Option<i64>,
    per_account: Option<i64>,
) -> ApiResult<()> {
    let (completed, reserved, local_max): (i64, i64, i64) = if kind == "VOUCHER" {
        let all: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM voucher_redemptions WHERE voucher_id=$1",
        )
        .bind(id)
        .fetch_one(&mut **transaction)
        .await?;
        let local: i64 = sqlx::query_scalar("SELECT COALESCE(max(uses),0)::bigint FROM (SELECT count(*) uses FROM voucher_redemptions WHERE voucher_id=$1 GROUP BY account_id) counts").bind(id).fetch_one(&mut **transaction).await?;
        (all, 0, local)
    } else {
        let row = sqlx::query("SELECT count(*) FILTER(WHERE status='COMPLETED')::bigint completed,count(*) FILTER(WHERE status='RESERVED')::bigint reserved FROM coupon_checkout_reservations WHERE coupon_id=$1").bind(id).fetch_one(&mut **transaction).await?;
        let local: i64 = sqlx::query_scalar("SELECT COALESCE(max(uses),0)::bigint FROM (SELECT count(*) uses FROM coupon_checkout_reservations WHERE coupon_id=$1 GROUP BY account_id) counts").bind(id).fetch_one(&mut **transaction).await?;
        (row.get("completed"), row.get("reserved"), local)
    };
    if total.is_some_and(|limit| limit < completed + reserved)
        || per_account.is_some_and(|limit| limit < local_max)
    {
        return Err(ApiError::conflict(
            "promotion_limit_below_usage",
            format!("{kind} {id} limits cannot be lower than completed and reserved uses"),
        ));
    }
    Ok(())
}

fn promotion_select(kind: &str) -> ApiResult<String> {
    Ok(match kind {
        "VOUCHER" => "SELECT p.voucher_id promotion_id,'VOUCHER' promotion_kind,p.code,p.name,p.description,p.status,p.version,p.valid_from,p.valid_until,p.max_total_uses,p.max_uses_per_account,p.credit_units,NULL::text discount_kind,NULL::bigint discount_value,NULL::text currency,NULL::boolean applies_to_initial,NULL::boolean applies_to_on_demand,p.created_at,p.updated_at,COALESCE((SELECT count(*) FROM voucher_redemptions r WHERE r.voucher_id=p.voucher_id),0)::bigint completed_uses,0::bigint reserved_uses,CASE WHEN p.status<>'ACTIVE' THEN p.status WHEN p.valid_from>statement_timestamp() THEN 'NOT_STARTED' WHEN p.valid_until<=statement_timestamp() THEN 'EXPIRED' WHEN p.max_total_uses IS NOT NULL AND (SELECT count(*) FROM voucher_redemptions r WHERE r.voucher_id=p.voucher_id)>=p.max_total_uses THEN 'EXHAUSTED' ELSE 'AVAILABLE' END availability FROM (SELECT vouchers.*,voucher_id promotion_id FROM vouchers) p".to_string(),
        "COUPON" => "SELECT p.coupon_id promotion_id,'COUPON' promotion_kind,p.code,p.name,p.description,p.status,p.version,p.valid_from,p.valid_until,p.max_total_uses,p.max_uses_per_account,NULL::bigint credit_units,p.discount_kind,p.discount_value,p.currency,p.applies_to_initial,p.applies_to_on_demand,p.created_at,p.updated_at,COALESCE((SELECT count(*) FROM coupon_checkout_reservations r WHERE r.coupon_id=p.coupon_id AND r.status='COMPLETED'),0)::bigint completed_uses,COALESCE((SELECT count(*) FROM coupon_checkout_reservations r WHERE r.coupon_id=p.coupon_id AND r.status='RESERVED'),0)::bigint reserved_uses,CASE WHEN p.status<>'ACTIVE' THEN p.status WHEN p.valid_from>statement_timestamp() THEN 'NOT_STARTED' WHEN p.valid_until<=statement_timestamp() THEN 'EXPIRED' WHEN p.max_total_uses IS NOT NULL AND (SELECT count(*) FROM coupon_checkout_reservations r WHERE r.coupon_id=p.coupon_id AND r.status IN ('COMPLETED','RESERVED'))>=p.max_total_uses THEN 'EXHAUSTED' ELSE 'AVAILABLE' END availability FROM (SELECT coupons.*,coupon_id promotion_id FROM coupons) p".to_string(),
        _ => return Err(invalid_kind(kind)),
    })
}

fn promotion_from_row(row: &sqlx::postgres::PgRow) -> PromotionResponse {
    PromotionResponse {
        promotion_id: row.get("promotion_id"),
        promotion_kind: row.get("promotion_kind"),
        code: row.get("code"),
        name: row.get("name"),
        description: row.get("description"),
        status: row.get("status"),
        version: row.get("version"),
        valid_from: row.get("valid_from"),
        valid_until: row.get("valid_until"),
        max_total_uses: row.get("max_total_uses"),
        max_uses_per_account: row.get("max_uses_per_account"),
        completed_uses: row.get("completed_uses"),
        reserved_uses: row.get("reserved_uses"),
        availability: row.get("availability"),
        credit_units: row
            .get::<Option<i64>, _>("credit_units")
            .map(CreditUnits::new),
        discount_kind: row.get("discount_kind"),
        discount_value: row.get("discount_value"),
        currency: row.get("currency"),
        applies_to_initial: row.get("applies_to_initial"),
        applies_to_on_demand: row.get("applies_to_on_demand"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

async fn load_promotion_json(
    transaction: &mut Transaction<'_, Postgres>,
    kind: &str,
    id: Uuid,
    lock: bool,
) -> ApiResult<Value> {
    let sql = format!(
        "SELECT to_jsonb(p) FROM {} p WHERE {}=$1{}",
        table_for(kind)?,
        id_column(kind)?,
        if lock { " FOR UPDATE" } else { "" }
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| {
            ApiError::not_found("promotion_not_found", format!("{kind} {id} does not exist"))
        })
}

struct PromotionHistorySnapshot<'a> {
    kind: &'a str,
    id: Uuid,
    version: i64,
    action: &'a str,
    actor: Option<&'a str>,
    before: Value,
    after: Value,
}

async fn insert_history(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot: PromotionHistorySnapshot<'_>,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO promotion_history (promotion_history_id,promotion_kind,promotion_id,version,action,actor_reference,before_snapshot,after_snapshot) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(Uuid::new_v4()).bind(snapshot.kind).bind(snapshot.id).bind(snapshot.version).bind(snapshot.action).bind(snapshot.actor).bind(snapshot.before).bind(snapshot.after).execute(&mut **transaction).await?;
    Ok(())
}

fn table_for(kind: &str) -> ApiResult<&'static str> {
    match kind {
        "VOUCHER" => Ok("vouchers"),
        "COUPON" => Ok("coupons"),
        _ => Err(invalid_kind(kind)),
    }
}
fn id_column(kind: &str) -> ApiResult<&'static str> {
    match kind {
        "VOUCHER" => Ok("voucher_id"),
        "COUPON" => Ok("coupon_id"),
        _ => Err(invalid_kind(kind)),
    }
}
fn invalid_kind(kind: &str) -> ApiError {
    ApiError::unprocessable(
        "invalid_promotion_kind",
        format!("promotion kind {kind:?} must be VOUCHER or COUPON"),
    )
}

fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}
fn parse_time(value: &str) -> ApiResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| {
            ApiError::unprocessable(
                "invalid_promotion_time",
                format!("validity timestamp {value:?} must be RFC3339"),
            )
        })
}

fn validate_editable_status(status: &str, before: &Value) -> ApiResult<()> {
    if before["status"] == "ARCHIVED" {
        return Err(ApiError::conflict(
            "promotion_archived",
            "archived promotions are terminal and cannot be edited",
        ));
    }
    if !matches!(status, "ACTIVE" | "DISABLED" | "ARCHIVED") {
        return Err(ApiError::unprocessable(
            "invalid_promotion_status",
            format!("status {status:?} must be ACTIVE, DISABLED, or terminal ARCHIVED"),
        ));
    }
    Ok(())
}
