use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::checkouts::{CheckoutKind, CheckoutResponse, CreateCheckoutRequest},
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

pub(super) struct CouponDiscount {
    pub(super) coupon_id: Uuid,
    pub(super) code: String,
    pub(super) version: i64,
    pub(super) kind: String,
    pub(super) value: i64,
    pub(super) base_amount_minor: i64,
    pub(super) discount_amount_minor: i64,
    pub(super) final_amount_minor: i64,
    pub(super) currency: String,
}

pub(super) async fn lock_coupon_discount(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    checkout_kind: CheckoutKind,
    code: &str,
    base_amount_minor: i64,
    currency: &str,
) -> ApiResult<CouponDiscount> {
    let canonical = code.trim().to_ascii_uppercase();
    let coupon = sqlx::query("SELECT * FROM coupons WHERE code=$1 FOR UPDATE")
        .bind(&canonical)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(
                "coupon_not_found",
                format!("coupon code {canonical:?} does not exist"),
            )
        })?;
    let id: Uuid = coupon.get("coupon_id");
    let status: String = coupon.get("status");
    let from: Option<DateTime<Utc>> = coupon.get("valid_from");
    let until: Option<DateTime<Utc>> = coupon.get("valid_until");
    let applicable: bool = match checkout_kind {
        CheckoutKind::Initial => coupon.get("applies_to_initial"),
        CheckoutKind::OnDemand => coupon.get("applies_to_on_demand"),
    };
    if status != "ACTIVE"
        || !applicable
        || from.is_some_and(|at| at > Utc::now())
        || until.is_some_and(|at| at <= Utc::now())
    {
        return Err(ApiError::conflict(
            "coupon_unavailable",
            format!(
                "coupon {id} is disabled, outside validity or not applicable to {checkout_kind:?}"
            ),
        ));
    }
    let total: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM coupon_checkout_reservations WHERE coupon_id=$1 AND status IN ('COMPLETED','RESERVED')")
        .bind(id).fetch_one(&mut **transaction).await?;
    let local: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM coupon_checkout_reservations WHERE coupon_id=$1 AND workspace_id=$2 AND status IN ('COMPLETED','RESERVED')")
        .bind(id).bind(workspace_id).fetch_one(&mut **transaction).await?;
    if coupon
        .get::<Option<i64>, _>("max_total_uses")
        .is_some_and(|limit| total >= limit)
        || coupon
            .get::<Option<i64>, _>("max_uses_per_workspace")
            .is_some_and(|limit| local >= limit)
    {
        return Err(ApiError::conflict(
            "coupon_usage_limit_reached",
            format!("coupon {id} has no remaining use for workspace {workspace_id}"),
        ));
    }
    let kind: String = coupon.get("discount_kind");
    let value: i64 = coupon.get("discount_value");
    let coupon_currency: Option<String> = coupon.get("currency");
    let discount = calculate_discount(
        base_amount_minor,
        kind.clone(),
        value,
        coupon_currency,
        currency,
    )?;
    if discount == 0 {
        return Err(ApiError::unprocessable(
            "coupon_discount_has_no_effect",
            format!("coupon {id} produces no discount after currency rounding"),
        ));
    }
    Ok(CouponDiscount {
        coupon_id: id,
        code: canonical,
        version: coupon.get("version"),
        kind,
        value,
        base_amount_minor,
        discount_amount_minor: discount,
        final_amount_minor: base_amount_minor - discount,
        currency: currency.to_string(),
    })
}

pub(super) async fn store_coupon_reservation(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    checkout_id: Uuid,
    collection_request_id: Uuid,
    checkout_kind: CheckoutKind,
    coupon: &CouponDiscount,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO coupon_checkout_reservations (coupon_checkout_reservation_id,coupon_id,workspace_id,checkout_id,collection_request_id,checkout_kind,base_amount_minor,discount_amount_minor,final_amount_minor,currency,discount_kind,discount_value,coupon_version,status) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'RESERVED')")
        .bind(Uuid::new_v4()).bind(coupon.coupon_id).bind(workspace_id).bind(checkout_id).bind(collection_request_id)
        .bind(checkout_kind_text(checkout_kind)).bind(coupon.base_amount_minor).bind(coupon.discount_amount_minor)
        .bind(coupon.final_amount_minor).bind(&coupon.currency).bind(&coupon.kind).bind(coupon.value).bind(coupon.version)
        .execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO promotion_usage_counters (promotion_kind,promotion_id,workspace_id,reserved_uses) VALUES ('COUPON',$1,$2,1) ON CONFLICT (promotion_kind,promotion_id,workspace_id) DO UPDATE SET reserved_uses=promotion_usage_counters.reserved_uses+1")
        .bind(coupon.coupon_id).bind(workspace_id).execute(&mut **transaction).await?;
    sqlx::query("UPDATE billing_checkouts SET coupon_id=$2,base_amount_minor=$3,discount_amount_minor=$4,checkout_currency=$5 WHERE checkout_id=$1")
        .bind(checkout_id).bind(coupon.coupon_id).bind(coupon.base_amount_minor).bind(coupon.discount_amount_minor).bind(&coupon.currency)
        .execute(&mut **transaction).await?;
    Ok(())
}

async fn store_free_coupon_redemption(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    checkout_id: Uuid,
    checkout_kind: CheckoutKind,
    coupon: &CouponDiscount,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO coupon_checkout_reservations (coupon_checkout_reservation_id,coupon_id,workspace_id,checkout_id,collection_request_id,checkout_kind,base_amount_minor,discount_amount_minor,final_amount_minor,currency,discount_kind,discount_value,coupon_version,status,completed_at) VALUES ($1,$2,$3,$4,NULL,$5,$6,$7,0,$8,$9,$10,$11,'COMPLETED',clock_timestamp())")
        .bind(Uuid::new_v4()).bind(coupon.coupon_id).bind(workspace_id).bind(checkout_id)
        .bind(checkout_kind_text(checkout_kind)).bind(coupon.base_amount_minor).bind(coupon.discount_amount_minor)
        .bind(&coupon.currency).bind(&coupon.kind).bind(coupon.value).bind(coupon.version)
        .execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO promotion_usage_counters (promotion_kind,promotion_id,workspace_id,completed_uses) VALUES ('COUPON',$1,$2,1) ON CONFLICT (promotion_kind,promotion_id,workspace_id) DO UPDATE SET completed_uses=promotion_usage_counters.completed_uses+1")
        .bind(coupon.coupon_id).bind(workspace_id).execute(&mut **transaction).await?;
    sqlx::query("UPDATE billing_checkouts SET coupon_id=$2,base_amount_minor=$3,discount_amount_minor=$4,checkout_currency=$5,completed_without_payment=true,lease_expires_at=NULL WHERE checkout_id=$1")
        .bind(checkout_id).bind(coupon.coupon_id).bind(coupon.base_amount_minor).bind(coupon.discount_amount_minor).bind(&coupon.currency)
        .execute(&mut **transaction).await?;
    Ok(())
}

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
    pub coupon_code: Option<String>,
    pub payment_method_binding_id: Option<Uuid>,
    pub completed_without_payment: bool,
    pub base_amount_minor: Option<i64>,
    pub discount_amount_minor: i64,
    pub checkout_currency: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl DatabaseRepository {
    pub async fn complete_free_checkout(
        &self,
        record: &CheckoutRecord,
        request: &CreateCheckoutRequest,
    ) -> ApiResult<()> {
        let code = request
            .coupon_code
            .as_deref()
            .ok_or_else(|| ApiError::unexpected("zero-price checkout requires a coupon"))?;
        let mut transaction = self.pool().begin().await?;
        let wallet =
            super::credits::lock_active_customer_wallet(&mut transaction, record.workspace_id)
                .await?;
        let completed: bool = sqlx::query_scalar("SELECT completed_without_payment FROM billing_checkouts WHERE checkout_id=$1 FOR UPDATE")
            .bind(record.checkout_id).fetch_one(&mut *transaction).await?;
        if completed {
            transaction.commit().await?;
            return Ok(());
        }
        let terms = lock_free_checkout_terms(&mut transaction, record).await?;
        let coupon = lock_coupon_discount(
            &mut transaction,
            record.workspace_id,
            record.checkout_kind,
            code,
            terms.base_amount_minor,
            &terms.currency,
        )
        .await?;
        if coupon.final_amount_minor != 0 {
            return Err(ApiError::conflict(
                "checkout_requires_payment",
                format!(
                    "coupon {} leaves {} minor units to collect",
                    code, coupon.final_amount_minor
                ),
            ));
        }
        let credit_entry_id = match record.checkout_kind {
            CheckoutKind::Initial => {
                let plan =
                    super::plan_cycles::load_locked_plan(&mut transaction, terms.plan_version_id)
                        .await?;
                super::plan_writes::ensure_recurring_credit_enabled(
                    &mut transaction,
                    record.workspace_id,
                    &plan,
                )
                .await?;
                let cycle = super::plan_writes::activate_customer_plan(
                    &mut transaction,
                    &wallet,
                    record.workspace_id,
                    record.customer_plan_id,
                    &plan,
                    terms.scheduled_at,
                    None,
                    &record.transaction_id,
                )
                .await?;
                let entry_id: Option<Uuid> = sqlx::query_scalar("SELECT customer_wallet_entry_id FROM wallet_transaction_references WHERE reference_kind='CUSTOMER_PLAN_CYCLE' AND customer_plan_cycle_id=$1")
                    .bind(cycle.customer_plan_cycle_id).fetch_optional(&mut *transaction).await?;
                sqlx::query("UPDATE customer_plans SET commercial_status='ACTIVE_PAID',activation_status='ACTIVATED',renewal_status='CURRENT',version=version+1 WHERE customer_plan_id=$1 AND activation_status='PENDING_INITIAL_PAYMENT'")
                    .bind(record.customer_plan_id).execute(&mut *transaction).await?;
                entry_id
            }
            CheckoutKind::OnDemand => {
                let grant = super::billing_on_demand_confirmation::OnDemandCreditGrant {
                    workspace_id: record.workspace_id,
                    on_demand_plan_id: terms.on_demand_plan_id,
                    plan_version_id: terms.plan_version_id,
                    granted_credit_units: terms.credit_units,
                    customer_plan_id: record.customer_plan_id,
                    transaction_id: &record.transaction_id,
                };
                let entry_id = super::billing_on_demand_confirmation::grant_on_demand_credit(
                    &mut transaction,
                    &wallet,
                    &grant,
                )
                .await?;
                Some(entry_id)
            }
        };
        if let Some(entry_id) = credit_entry_id {
            sqlx::query("INSERT INTO wallet_transaction_references (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind,coupon_id) VALUES ($1,$2,'COUPON',$3)")
                .bind(Uuid::new_v4()).bind(entry_id).bind(coupon.coupon_id).execute(&mut *transaction).await?;
        }
        store_free_coupon_redemption(
            &mut transaction,
            record.workspace_id,
            record.checkout_id,
            record.checkout_kind,
            &coupon,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn checkout_credit_units(&self, record: &CheckoutRecord) -> ApiResult<i64> {
        let credits = match record.checkout_kind {
            CheckoutKind::Initial => sqlx::query_scalar("SELECT p.granted_credit_units FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) WHERE cp.customer_plan_id=$1 AND cp.customer_id=$2")
                .bind(record.customer_plan_id).bind(record.workspace_id).fetch_optional(&self.pool()).await?,
            CheckoutKind::OnDemand => sqlx::query_scalar("SELECT od.credit_units FROM on_demand_plans od WHERE od.on_demand_plan_id=$1")
                .bind(record.on_demand_plan_id).fetch_optional(&self.pool()).await?,
        };
        credits.ok_or_else(|| {
            ApiError::not_found(
                "checkout_benefit_not_found",
                format!(
                    "benefit for checkout {} no longer resolves",
                    record.checkout_id
                ),
            )
        })
    }

    pub async fn quote_checkout(
        &self,
        workspace_id: Uuid,
        request: &crate::dto::checkouts::CheckoutQuoteRequest,
    ) -> ApiResult<crate::dto::checkouts::CheckoutQuoteResponse> {
        let terms = quote_terms(self, workspace_id, request).await?;
        let coupon = sqlx::query("SELECT * FROM coupons WHERE code=$1 FOR SHARE")
            .bind(request.coupon_code.trim().to_ascii_uppercase())
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| ApiError::not_found("coupon_not_found", "coupon code does not exist"))?;
        let coupon_id: Uuid = coupon.get("coupon_id");
        validate_coupon_for_quote(&self.pool(), &coupon, workspace_id, request, terms.1).await?;
        let discount = calculate_discount(
            terms.0,
            coupon.get("discount_kind"),
            coupon.get("discount_value"),
            coupon.get("currency"),
            &terms.2,
        )?;
        if discount == 0 {
            return Err(ApiError::unprocessable(
                "coupon_discount_has_no_effect",
                format!("coupon {coupon_id} produces no discount after currency rounding"),
            ));
        }
        let final_amount = terms.0 - discount;
        Ok(crate::dto::checkouts::CheckoutQuoteResponse {
            customer_plan_id: request.customer_plan_id,
            checkout_kind: request.checkout_kind,
            base_amount_minor: terms.0,
            discount_amount_minor: discount,
            amount_minor: final_amount,
            currency: terms.2,
            granted_credit_units: terms.3,
            coupon_id,
            coupon_code: coupon.get("code"),
            coupon_version: coupon.get("version"),
            payment_required: final_amount > 0,
        })
    }

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
        if record.collection_request_id.is_some() || record.completed_without_payment {
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
            "UPDATE billing_checkouts SET collection_request_id=$2,lease_expires_at=NULL, \
             base_amount_minor=COALESCE(base_amount_minor,(SELECT COALESCE(base_amount_minor,amount_minor) FROM collection_requests WHERE collection_request_id=$2)), \
             discount_amount_minor=COALESCE((SELECT discount_amount_minor FROM collection_requests WHERE collection_request_id=$2),0), \
             checkout_currency=COALESCE(checkout_currency,(SELECT currency FROM collection_requests WHERE collection_request_id=$2)) \
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
            base_amount_minor: record.base_amount_minor,
            discount_amount_minor: record.discount_amount_minor,
            coupon_code: record.coupon_code,
            payment_required: collection.is_some_and(|item| item.amount_minor > 0),
        }
    }
}

struct FreeCheckoutTerms {
    plan_version_id: Uuid,
    on_demand_plan_id: Option<Uuid>,
    base_amount_minor: i64,
    currency: String,
    credit_units: i64,
    scheduled_at: DateTime<Utc>,
}

async fn lock_free_checkout_terms(
    transaction: &mut Transaction<'_, Postgres>,
    record: &CheckoutRecord,
) -> ApiResult<FreeCheckoutTerms> {
    let row = match record.checkout_kind {
        CheckoutKind::Initial => sqlx::query("SELECT cp.plan_version_id,p.price_amount_minor,p.currency,p.granted_credit_units,statement_timestamp() scheduled_at FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) WHERE cp.customer_plan_id=$1 AND cp.customer_id=$2 AND cp.commercial_status='ACTIVE' AND cp.activation_status='PENDING_INITIAL_PAYMENT' AND p.commercial_model='PAID' AND p.revoked_at IS NULL FOR UPDATE OF cp,p")
            .bind(record.customer_plan_id).bind(record.workspace_id).fetch_optional(&mut **transaction).await?,
        CheckoutKind::OnDemand => sqlx::query("SELECT cp.plan_version_id,od.on_demand_plan_id,od.price_amount_minor,od.currency,od.credit_units granted_credit_units,statement_timestamp() scheduled_at FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) JOIN on_demand_plans od USING(subscription_id) WHERE cp.customer_plan_id=$1 AND cp.customer_id=$2 AND od.on_demand_plan_id=$3 AND cp.commercial_status IN ('ACTIVE','ACTIVE_PAID') AND cp.activation_status='ACTIVATED' AND cp.renewal_status='CURRENT' AND p.revoked_at IS NULL AND od.revoked_at IS NULL FOR UPDATE OF cp,p,od")
            .bind(record.customer_plan_id).bind(record.workspace_id).bind(record.on_demand_plan_id).fetch_optional(&mut **transaction).await?,
    }.ok_or_else(|| ApiError::conflict("checkout_not_eligible", format!("customer plan {} is not eligible for free checkout",record.customer_plan_id)))?;
    Ok(FreeCheckoutTerms {
        plan_version_id: row.get("plan_version_id"),
        on_demand_plan_id: row.try_get("on_demand_plan_id").ok(),
        base_amount_minor: row.get("price_amount_minor"),
        currency: row.get("currency"),
        credit_units: row.get("granted_credit_units"),
        scheduled_at: row.get("scheduled_at"),
    })
}

async fn quote_terms(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    request: &crate::dto::checkouts::CheckoutQuoteRequest,
) -> ApiResult<(i64, bool, String, i64)> {
    let row = match request.checkout_kind {
        CheckoutKind::Initial => {
            if request.on_demand_plan_id.is_some() {
                return Err(invalid_quote_shape(request));
            }
            sqlx::query("SELECT p.price_amount_minor amount_minor,p.currency,p.granted_credit_units FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) WHERE cp.customer_plan_id=$1 AND cp.customer_id=$2 AND cp.commercial_status='ACTIVE' AND cp.activation_status='PENDING_INITIAL_PAYMENT' AND p.commercial_model='PAID' AND p.revoked_at IS NULL")
                .bind(request.customer_plan_id).bind(workspace_id).fetch_optional(&repository.pool()).await?
        }
        CheckoutKind::OnDemand => {
            let Some(plan_id) = request.on_demand_plan_id else { return Err(invalid_quote_shape(request)); };
            sqlx::query("SELECT od.price_amount_minor amount_minor,od.currency,od.credit_units granted_credit_units FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) JOIN on_demand_plans od USING(subscription_id) WHERE cp.customer_plan_id=$1 AND cp.customer_id=$2 AND od.on_demand_plan_id=$3 AND cp.commercial_status IN ('ACTIVE','ACTIVE_PAID') AND cp.activation_status='ACTIVATED' AND cp.renewal_status='CURRENT' AND p.revoked_at IS NULL AND od.revoked_at IS NULL")
                .bind(request.customer_plan_id).bind(workspace_id).bind(plan_id).fetch_optional(&repository.pool()).await?
        }
    }.ok_or_else(|| ApiError::conflict("checkout_quote_not_eligible", format!("customer plan {} is not eligible for {:?} checkout",request.customer_plan_id,request.checkout_kind)))?;
    Ok((
        row.get("amount_minor"),
        request.checkout_kind == CheckoutKind::Initial,
        row.get("currency"),
        row.get("granted_credit_units"),
    ))
}

async fn validate_coupon_for_quote(
    pool: &sqlx::PgPool,
    coupon: &sqlx::postgres::PgRow,
    workspace_id: Uuid,
    request: &crate::dto::checkouts::CheckoutQuoteRequest,
    initial: bool,
) -> ApiResult<()> {
    let id: Uuid = coupon.get("coupon_id");
    let status: String = coupon.get("status");
    let from: Option<DateTime<Utc>> = coupon.get("valid_from");
    let until: Option<DateTime<Utc>> = coupon.get("valid_until");
    let applies: bool = if initial {
        coupon.get("applies_to_initial")
    } else {
        coupon.get("applies_to_on_demand")
    };
    if status != "ACTIVE"
        || !applies
        || from.is_some_and(|at| at > Utc::now())
        || until.is_some_and(|at| at <= Utc::now())
    {
        return Err(ApiError::conflict(
            "coupon_unavailable",
            format!(
                "coupon {id} is disabled, outside validity or not applicable to {:?}",
                request.checkout_kind
            ),
        ));
    }
    let total: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM coupon_checkout_reservations WHERE coupon_id=$1 AND status IN ('COMPLETED','RESERVED')")
        .bind(id).fetch_one(pool).await?;
    let local: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM coupon_checkout_reservations WHERE coupon_id=$1 AND workspace_id=$2 AND status IN ('COMPLETED','RESERVED')")
        .bind(id).bind(workspace_id).fetch_one(pool).await?;
    if coupon
        .get::<Option<i64>, _>("max_total_uses")
        .is_some_and(|limit| total >= limit)
    {
        return Err(ApiError::conflict(
            "coupon_usage_limit_reached",
            format!("coupon {id} total usage limit reached"),
        ));
    }
    if coupon
        .get::<Option<i64>, _>("max_uses_per_workspace")
        .is_some_and(|limit| local >= limit)
    {
        return Err(ApiError::conflict(
            "coupon_usage_limit_reached",
            format!("coupon {id} workspace usage limit reached"),
        ));
    }
    Ok(())
}

fn calculate_discount(
    base: i64,
    kind: String,
    value: i64,
    currency: Option<String>,
    actual_currency: &str,
) -> ApiResult<i64> {
    let amount = match kind.as_str() {
        "PERCENTAGE" => (i128::from(base) * i128::from(value) / 10_000) as i64,
        "FIXED" if currency.as_deref() == Some(actual_currency) => value,
        "FIXED" => {
            return Err(ApiError::conflict(
                "coupon_currency_mismatch",
                format!(
                    "coupon currency {:?} does not match checkout currency {actual_currency}",
                    currency
                ),
            ));
        }
        _ => {
            return Err(ApiError::conflict(
                "coupon_discount_invalid",
                format!("coupon discount kind {kind:?} is unsupported"),
            ));
        }
    };
    Ok(amount.min(base))
}

fn invalid_quote_shape(request: &crate::dto::checkouts::CheckoutQuoteRequest) -> ApiError {
    ApiError::unprocessable(
        "invalid_checkout_quote",
        format!(
            "checkout kind {:?} must have a matching on-demand plan identifier",
            request.checkout_kind
        ),
    )
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
         on_demand_plan_id,transaction_id,idempotency_key,request_sha256,coupon_code,payment_method_binding_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(request.customer_plan_id)
    .bind(checkout_kind_text(request.checkout_kind))
    .bind(request.on_demand_plan_id)
    .bind(&request.transaction_id)
    .bind(key)
    .bind(request_hash)
    .bind(request.coupon_code.as_deref())
    .bind(request.payment_method_binding_id)
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
        coupon_code: row.get("coupon_code"),
        payment_method_binding_id: row.get("payment_method_binding_id"),
        completed_without_payment: row.get("completed_without_payment"),
        base_amount_minor: row.get("base_amount_minor"),
        discount_amount_minor: row.get("discount_amount_minor"),
        checkout_currency: row.get("checkout_currency"),
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

#[cfg(test)]
mod tests {
    use super::calculate_discount;

    #[test]
    fn percentage_discount_rounds_down_in_minor_units() {
        assert_eq!(
            calculate_discount(101, "PERCENTAGE".into(), 3_333, None, "USD").unwrap(),
            33
        );
    }

    #[test]
    fn full_percentage_and_large_fixed_discount_are_capped_at_price() {
        assert_eq!(
            calculate_discount(500, "PERCENTAGE".into(), 10_000, None, "USD").unwrap(),
            500
        );
        assert_eq!(
            calculate_discount(500, "FIXED".into(), 900, Some("USD".into()), "USD").unwrap(),
            500
        );
    }

    #[test]
    fn fixed_discount_requires_the_checkout_currency() {
        assert!(calculate_discount(500, "FIXED".into(), 100, Some("EUR".into()), "USD").is_err());
    }
}
