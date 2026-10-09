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

pub struct HostedCheckoutRecovery {
    pub billing_connection_id: Uuid,
    pub collection: crate::dto::billing::CollectionRequestResponse,
}

pub(super) async fn lock_coupon_discount(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
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
        CheckoutKind::PlanUpgrade => false,
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
    let local: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM coupon_checkout_reservations WHERE coupon_id=$1 AND account_id=$2 AND status IN ('COMPLETED','RESERVED')")
        .bind(id).bind(account_id).fetch_one(&mut **transaction).await?;
    if coupon
        .get::<Option<i64>, _>("max_total_uses")
        .is_some_and(|limit| total >= limit)
        || coupon
            .get::<Option<i64>, _>("max_uses_per_account")
            .is_some_and(|limit| local >= limit)
    {
        return Err(ApiError::conflict(
            "coupon_usage_limit_reached",
            format!("coupon {id} has no remaining use for account {account_id}"),
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
    account_id: Uuid,
    checkout_id: Uuid,
    collection_request_id: Uuid,
    checkout_kind: CheckoutKind,
    coupon: &CouponDiscount,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO coupon_checkout_reservations (coupon_checkout_reservation_id,coupon_id,account_id,checkout_id,collection_request_id,checkout_kind,base_amount_minor,discount_amount_minor,final_amount_minor,currency,discount_kind,discount_value,coupon_version,status) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'RESERVED')")
        .bind(Uuid::new_v4()).bind(coupon.coupon_id).bind(account_id).bind(checkout_id).bind(collection_request_id)
        .bind(checkout_kind_text(checkout_kind)).bind(coupon.base_amount_minor).bind(coupon.discount_amount_minor)
        .bind(coupon.final_amount_minor).bind(&coupon.currency).bind(&coupon.kind).bind(coupon.value).bind(coupon.version)
        .execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO promotion_usage_counters (promotion_kind,promotion_id,account_id,reserved_uses) VALUES ('COUPON',$1,$2,1) ON CONFLICT (promotion_kind,promotion_id,account_id) DO UPDATE SET reserved_uses=promotion_usage_counters.reserved_uses+1")
        .bind(coupon.coupon_id).bind(account_id).execute(&mut **transaction).await?;
    sqlx::query("UPDATE billing_checkouts SET coupon_id=$2,base_amount_minor=$3,discount_amount_minor=$4,checkout_currency=$5 WHERE checkout_id=$1")
        .bind(checkout_id).bind(coupon.coupon_id).bind(coupon.base_amount_minor).bind(coupon.discount_amount_minor).bind(&coupon.currency)
        .execute(&mut **transaction).await?;
    Ok(())
}

async fn store_free_coupon_redemption(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    checkout_id: Uuid,
    checkout_kind: CheckoutKind,
    coupon: &CouponDiscount,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO coupon_checkout_reservations (coupon_checkout_reservation_id,coupon_id,account_id,checkout_id,collection_request_id,checkout_kind,base_amount_minor,discount_amount_minor,final_amount_minor,currency,discount_kind,discount_value,coupon_version,status,completed_at) VALUES ($1,$2,$3,$4,NULL,$5,$6,$7,0,$8,$9,$10,$11,'COMPLETED',clock_timestamp())")
        .bind(Uuid::new_v4()).bind(coupon.coupon_id).bind(account_id).bind(checkout_id)
        .bind(checkout_kind_text(checkout_kind)).bind(coupon.base_amount_minor).bind(coupon.discount_amount_minor)
        .bind(&coupon.currency).bind(&coupon.kind).bind(coupon.value).bind(coupon.version)
        .execute(&mut **transaction).await?;
    sqlx::query("INSERT INTO promotion_usage_counters (promotion_kind,promotion_id,account_id,completed_uses) VALUES ('COUPON',$1,$2,1) ON CONFLICT (promotion_kind,promotion_id,account_id) DO UPDATE SET completed_uses=promotion_usage_counters.completed_uses+1")
        .bind(coupon.coupon_id).bind(account_id).execute(&mut **transaction).await?;
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
    pub account_id: Uuid,
    pub customer_plan_id: Uuid,
    pub checkout_kind: CheckoutKind,
    pub on_demand_plan_id: Option<Uuid>,
    pub target_plan_version_id: Option<Uuid>,
    pub transaction_id: String,
    pub idempotency_key: String,
    pub request_sha256: String,
    pub collection_request_id: Option<Uuid>,
    pub coupon_code: Option<String>,
    pub payment_method_binding_id: Option<Uuid>,
    pub save_payment_method: bool,
    pub completed_without_payment: bool,
    pub base_amount_minor: Option<i64>,
    pub discount_amount_minor: i64,
    pub checkout_currency: Option<String>,
    pub quantity: i32,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl DatabaseRepository {
    pub async fn checkout_id_for_collection(&self, collection_id: Uuid) -> ApiResult<Option<Uuid>> {
        Ok(sqlx::query_scalar(
            "SELECT checkout_id FROM billing_checkouts WHERE collection_request_id=$1",
        )
        .bind(collection_id)
        .fetch_optional(&self.pool())
        .await?)
    }

    pub async fn recoverable_hosted_checkout(
        &self,
        checkout_id: Uuid,
    ) -> ApiResult<Option<HostedCheckoutRecovery>> {
        let row = sqlx::query(
            "UPDATE collection_requests cr SET payment_expires_at=GREATEST( \
               cr.payment_expires_at,cr.scheduled_at+interval '23 hours') \
             FROM billing_hosted_payment_sessions hs WHERE hs.checkout_id=$1 \
               AND hs.collection_request_id=cr.collection_request_id AND hs.status='CREATING' \
               AND hs.provider_session_id IS NULL RETURNING hs.billing_connection_id,cr.*",
        )
        .bind(checkout_id)
        .fetch_optional(&self.pool())
        .await?;
        Ok(row.map(|row| HostedCheckoutRecovery {
            billing_connection_id: row.get("billing_connection_id"),
            collection: super::billing_regularization::collection_from_row(&row),
        }))
    }

    // Stripe rejects Checkout Sessions below 30 minutes and caps their lifetime at 24 hours.
    pub async fn hosted_checkout_collection(
        &self,
        checkout_id: Uuid,
    ) -> ApiResult<crate::dto::billing::CollectionRequestResponse> {
        let row = sqlx::query(
            "UPDATE collection_requests cr SET payment_expires_at=GREATEST( \
               cr.payment_expires_at,cr.scheduled_at+interval '23 hours') \
             FROM billing_hosted_payment_sessions hs WHERE hs.checkout_id=$1 \
               AND hs.collection_request_id=cr.collection_request_id RETURNING cr.*",
        )
        .bind(checkout_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| {
            ApiError::not_found(
                "hosted_checkout_not_found",
                format!("checkout {checkout_id} has no linked collection"),
            )
        })?;
        Ok(super::billing_regularization::collection_from_row(&row))
    }

    pub async fn confirm_hosted_payment(
        &self,
        checkout_id: Uuid,
        provider_session_id: &str,
        provider_payment_id: &str,
        payment_method_id: &str,
        save_payment_method: bool,
        amount_minor: i64,
        currency: &str,
        card: Option<&crate::repositories::stripe::StripeCardSummary>,
    ) -> ApiResult<HostedCheckoutConfirmation> {
        let mut transaction = self.pool().begin().await?;
        let row = sqlx::query(
            "SELECT hs.collection_request_id,hs.provider_session_id,hs.payment_method_binding_id, \
             cr.account_id,cr.amount_minor,cr.currency,c.save_payment_method,c.checkout_kind \
             FROM billing_hosted_payment_sessions hs \
             JOIN collection_requests cr ON cr.collection_request_id=hs.collection_request_id \
             JOIN billing_checkouts c ON c.checkout_id=hs.checkout_id \
             WHERE hs.checkout_id=$1 FOR UPDATE OF hs,cr",
        )
        .bind(checkout_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(
                "hosted_checkout_not_found",
                format!("checkout {checkout_id} is not linked to a payment"),
            )
        })?;
        let stored_session: String = row.get("provider_session_id");
        let stored_amount: i64 = row.get("amount_minor");
        let stored_currency: String = row.get("currency");
        if stored_session != provider_session_id
            || stored_amount != amount_minor
            || stored_currency != currency
            || provider_payment_id.is_empty()
            || payment_method_id.is_empty()
            || (save_payment_method
                && !row.get::<bool, _>("save_payment_method")
                && row.get::<String, _>("checkout_kind") != "INITIAL")
        {
            return Err(ApiError::conflict("hosted_payment_snapshot_mismatch", format!("hosted session {provider_session_id} does not match checkout {checkout_id} snapshot")));
        }
        if save_payment_method {
            sqlx::query("UPDATE payment_method_bindings SET provider_payment_method_reference=$2,status='ACTIVE',card_brand=$3,card_last_four=$4,card_exp_month=$5,card_exp_year=$6 WHERE payment_method_binding_id=$1 AND status IN ('ACTIVE','PENDING')")
                .bind(row.get::<Uuid, _>("payment_method_binding_id")).bind(payment_method_id)
                .bind(card.map(|value| value.brand.as_str())).bind(card.map(|value| value.last_four.as_str()))
                .bind(card.and_then(|value| i16::try_from(value.exp_month).ok()))
                .bind(card.and_then(|value| i16::try_from(value.exp_year).ok()))
                .execute(&mut *transaction).await?;
        } else {
            sqlx::query("UPDATE payment_method_bindings SET status='DETACHED' WHERE payment_method_binding_id=$1 AND status IN ('ACTIVE','PENDING')")
                .bind(row.get::<Uuid, _>("payment_method_binding_id")).execute(&mut *transaction).await?;
        }
        sqlx::query("UPDATE billing_payments SET provider_payment_id=$2 WHERE collection_request_id=$1 AND state='PENDING'")
            .bind(row.get::<Uuid, _>("collection_request_id")).bind(provider_payment_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE billing_hosted_payment_sessions SET status='COMPLETED' WHERE checkout_id=$1 AND status IN ('OPEN','COMPLETED')")
            .bind(checkout_id).execute(&mut *transaction).await?;
        let confirmation = HostedCheckoutConfirmation {
            collection_request_id: row.get("collection_request_id"),
            account_id: row.get("account_id"),
            amount_minor: stored_amount,
            currency: stored_currency,
        };
        transaction.commit().await?;
        Ok(confirmation)
    }

    pub async fn ensure_hosted_payment_binding(
        &self,
        checkout_id: Uuid,
        account_id: Uuid,
        connection_id: Uuid,
    ) -> ApiResult<Uuid> {
        let mut transaction = self.pool().begin().await?;
        if let Some(binding_id) = sqlx::query_scalar(
            "SELECT h.payment_method_binding_id FROM billing_hosted_payment_sessions h \
             JOIN billing_checkouts c USING(checkout_id) WHERE h.checkout_id=$1 AND c.account_id=$2 FOR UPDATE OF h",
        )
        .bind(checkout_id)
        .bind(account_id)
        .fetch_optional(&mut *transaction)
        .await?
        {
            transaction.commit().await?;
            return Ok(binding_id);
        }
        let binding_id = Uuid::new_v4();
        let placeholder = format!("pending_checkout_{checkout_id}");
        sqlx::query(
            "INSERT INTO payment_method_bindings (payment_method_binding_id,billing_connection_id, \
             account_id,customer_id,customer_plan_id,payment_method,provider_payment_method_reference,status) \
             SELECT $1,$2,account_id,account_id,customer_plan_id,'CARD',$3,'ACTIVE' \
             FROM billing_checkouts WHERE checkout_id=$4 AND account_id=$5",
        )
        .bind(binding_id)
        .bind(connection_id)
        .bind(placeholder)
        .bind(checkout_id)
        .bind(account_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO billing_hosted_payment_sessions \
             (checkout_id,billing_connection_id,payment_method_binding_id,status) VALUES ($1,$2,$3,'CREATING')",
        )
        .bind(checkout_id)
        .bind(connection_id)
        .bind(binding_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE billing_checkouts SET payment_method_binding_id=$2 WHERE checkout_id=$1",
        )
        .bind(checkout_id)
        .bind(binding_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(binding_id)
    }

    pub async fn name_hosted_checkout_payment_method(
        &self,
        checkout_id: Uuid,
        display_name: Option<&str>,
    ) -> ApiResult<()> {
        sqlx::query(
            "UPDATE payment_method_bindings pmb SET display_name=$2 FROM billing_checkouts c \
             WHERE c.checkout_id=$1 AND c.payment_method_binding_id=pmb.payment_method_binding_id \
             AND c.save_payment_method=true AND pmb.status='ACTIVE'",
        )
        .bind(checkout_id)
        .bind(display_name)
        .execute(&self.pool())
        .await?;
        Ok(())
    }

    pub async fn mark_hosted_collection_pending(
        &self,
        checkout_id: Uuid,
        collection_id: Uuid,
    ) -> ApiResult<()> {
        let mut transaction = self.pool().begin().await?;
        let row = sqlx::query(
            "SELECT cr.amount_minor,cr.currency,cr.scheduled_at,cr.status,hs.billing_connection_id \
             FROM collection_requests cr JOIN billing_hosted_payment_sessions hs USING(payment_method_binding_id) \
             WHERE cr.collection_request_id=$1 AND hs.checkout_id=$2 FOR UPDATE OF cr,hs",
        )
        .bind(collection_id)
        .bind(checkout_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| ApiError::not_found(
            "hosted_checkout_not_found",
            format!("checkout {checkout_id} has no collection {collection_id}"),
        ))?;
        if row.get::<String, _>("status") != "SCHEDULED" {
            transaction.commit().await?;
            return Ok(());
        }
        sqlx::query("UPDATE billing_hosted_payment_sessions SET collection_request_id=$2 WHERE checkout_id=$1 AND collection_request_id IS NULL")
            .bind(checkout_id).bind(collection_id).execute(&mut *transaction).await?;
        let attempt_id = Uuid::new_v4();
        let provider: String = sqlx::query_scalar(
            "SELECT provider FROM billing_connections WHERE billing_connection_id=$1",
        )
        .bind(row.get::<Uuid, _>("billing_connection_id"))
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO collection_attempts (collection_attempt_id,collection_request_id,attempt_number, \
             connector,payment_method,provider_idempotency_key,status,scheduled_at,started_at) \
             VALUES ($1,$2,1,$3,'CARD',$4,'PENDING',$5,clock_timestamp())",
        )
        .bind(attempt_id)
        .bind(collection_id)
        .bind(&provider)
        .bind(format!("hosted:{collection_id}:attempt:1"))
        .bind(row.get::<DateTime<Utc>, _>("scheduled_at"))
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO billing_payments (billing_payment_id,collection_request_id,collection_attempt_id, \
             provider,state,amount_minor,currency) VALUES ($1,$2,$3,$4,'PENDING',$5,$6)",
        )
        .bind(Uuid::new_v4())
        .bind(collection_id)
        .bind(attempt_id)
        .bind(provider)
        .bind(row.get::<i64, _>("amount_minor"))
        .bind(row.get::<String, _>("currency"))
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE collection_requests SET status='PENDING_PAYMENT',attempts_started=1 WHERE collection_request_id=$1")
            .bind(collection_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE payment_method_bindings SET status='PENDING' WHERE payment_method_binding_id=(SELECT payment_method_binding_id FROM billing_hosted_payment_sessions WHERE checkout_id=$1)")
            .bind(checkout_id).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn finish_hosted_payment_session(
        &self,
        checkout_id: Uuid,
        provider_session_id: &str,
        redirect_url: &str,
    ) -> ApiResult<()> {
        sqlx::query(
            "UPDATE billing_hosted_payment_sessions SET provider_session_id=$2,redirect_url=$3,status='OPEN' \
             WHERE checkout_id=$1 AND status IN ('CREATING','OPEN')",
        )
        .bind(checkout_id)
        .bind(provider_session_id)
        .bind(redirect_url)
        .execute(&self.pool())
        .await?;
        Ok(())
    }

    pub async fn hosted_payment_redirect_url(
        &self,
        checkout_id: Uuid,
    ) -> ApiResult<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT redirect_url FROM billing_hosted_payment_sessions WHERE checkout_id=$1 AND status='OPEN'",
        )
        .bind(checkout_id)
        .fetch_optional(&self.pool())
        .await?)
    }

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
            super::credits::lock_active_customer_wallet(&mut transaction, record.account_id)
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
            record.account_id,
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
                    record.account_id,
                    &plan,
                )
                .await?;
                let cycle = super::plan_writes::activate_customer_plan(
                    &mut transaction,
                    &wallet,
                    record.account_id,
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
                    account_id: record.account_id,
                    on_demand_plan_id: terms.on_demand_plan_id,
                    plan_version_id: terms.plan_version_id,
                    granted_credit_units: terms.credit_units,
                    quantity: record.quantity,
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
            CheckoutKind::PlanUpgrade => {
                return Err(ApiError::unexpected(
                    "upgrade checkouts cannot use free coupon completion",
                ))
            }
        };
        if let Some(entry_id) = credit_entry_id {
            sqlx::query("INSERT INTO wallet_transaction_references (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind,coupon_id) VALUES ($1,$2,'COUPON',$3)")
                .bind(Uuid::new_v4()).bind(entry_id).bind(coupon.coupon_id).execute(&mut *transaction).await?;
        }
        store_free_coupon_redemption(
            &mut transaction,
            record.account_id,
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
                .bind(record.customer_plan_id).bind(record.account_id).fetch_optional(&self.pool()).await?,
            CheckoutKind::OnDemand => sqlx::query_scalar("SELECT od.credit_units FROM on_demand_plans od WHERE od.on_demand_plan_id=$1")
                .bind(record.on_demand_plan_id).fetch_optional(&self.pool()).await?,
            CheckoutKind::PlanUpgrade => sqlx::query_scalar("SELECT granted_credit_units FROM customer_plan_transitions WHERE customer_plan_id=$1 AND new_plan_version_id=$2 ORDER BY created_at DESC LIMIT 1")
                .bind(record.customer_plan_id).bind(record.target_plan_version_id).fetch_optional(&self.pool()).await?,
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
        account_id: Uuid,
        request: &crate::dto::checkouts::CheckoutQuoteRequest,
    ) -> ApiResult<crate::dto::checkouts::CheckoutQuoteResponse> {
        let terms = quote_terms(self, account_id, request).await?;
        let coupon = sqlx::query("SELECT * FROM coupons WHERE code=$1 FOR SHARE")
            .bind(request.coupon_code.trim().to_ascii_uppercase())
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| ApiError::not_found("coupon_not_found", "coupon code does not exist"))?;
        let coupon_id: Uuid = coupon.get("coupon_id");
        validate_coupon_for_quote(&self.pool(), &coupon, account_id, request, terms.1).await?;
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
        account_id: Uuid,
        key: &str,
        request: &CreateCheckoutRequest,
        request_hash: &str,
    ) -> ApiResult<CheckoutClaim> {
        let mut transaction = self.pool().begin().await?;
        lock_checkout_key(&mut transaction, account_id, key).await?;
        let record = match find_by_key(&mut transaction, account_id, key).await? {
            Some(record) => {
                validate_same_request(&record, request_hash)?;
                record
            }
            None => {
                insert_checkout(&mut transaction, account_id, key, request, request_hash).await?
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
        account_id: Option<Uuid>,
    ) -> ApiResult<CheckoutRecord> {
        let row = sqlx::query(
            "SELECT * FROM billing_checkouts WHERE checkout_id=$1 AND ($2::uuid IS NULL OR account_id=$2)",
        )
        .bind(checkout_id)
        .bind(account_id)
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
            redirect_url: None,
        }
    }
}

pub struct HostedCheckoutConfirmation {
    pub collection_request_id: Uuid,
    pub account_id: Uuid,
    pub amount_minor: i64,
    pub currency: String,
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
            .bind(record.customer_plan_id).bind(record.account_id).fetch_optional(&mut **transaction).await?,
        CheckoutKind::OnDemand => sqlx::query("SELECT cp.plan_version_id,od.on_demand_plan_id,od.price_amount_minor,od.currency,od.credit_units granted_credit_units,statement_timestamp() scheduled_at FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) JOIN on_demand_plans od USING(subscription_id) WHERE cp.customer_plan_id=$1 AND cp.customer_id=$2 AND od.on_demand_plan_id=$3 AND cp.commercial_status IN ('ACTIVE','ACTIVE_PAID') AND cp.activation_status='ACTIVATED' AND cp.renewal_status='CURRENT' AND p.revoked_at IS NULL AND od.revoked_at IS NULL FOR UPDATE OF cp,p,od")
            .bind(record.customer_plan_id).bind(record.account_id).bind(record.on_demand_plan_id).fetch_optional(&mut **transaction).await?,
        CheckoutKind::PlanUpgrade => return Err(ApiError::unexpected("plan upgrade checkout cannot be completed without payment")),
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
    account_id: Uuid,
    request: &crate::dto::checkouts::CheckoutQuoteRequest,
) -> ApiResult<(i64, bool, String, i64)> {
    let row = match request.checkout_kind {
        CheckoutKind::Initial => {
            if request.on_demand_plan_id.is_some() {
                return Err(invalid_quote_shape(request));
            }
            sqlx::query("SELECT p.price_amount_minor amount_minor,p.currency,p.granted_credit_units FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) WHERE cp.customer_plan_id=$1 AND cp.customer_id=$2 AND cp.commercial_status='ACTIVE' AND cp.activation_status='PENDING_INITIAL_PAYMENT' AND p.commercial_model='PAID' AND p.revoked_at IS NULL")
                .bind(request.customer_plan_id).bind(account_id).fetch_optional(&repository.pool()).await?
        }
        CheckoutKind::OnDemand => {
            let Some(plan_id) = request.on_demand_plan_id else { return Err(invalid_quote_shape(request)); };
            sqlx::query("SELECT od.price_amount_minor amount_minor,od.currency,od.credit_units granted_credit_units FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) JOIN on_demand_plans od USING(subscription_id) WHERE cp.customer_plan_id=$1 AND cp.customer_id=$2 AND od.on_demand_plan_id=$3 AND cp.commercial_status IN ('ACTIVE','ACTIVE_PAID') AND cp.activation_status='ACTIVATED' AND cp.renewal_status='CURRENT' AND p.revoked_at IS NULL AND od.revoked_at IS NULL")
                .bind(request.customer_plan_id).bind(account_id).bind(plan_id).fetch_optional(&repository.pool()).await?
        }
        CheckoutKind::PlanUpgrade => return Err(invalid_quote_shape(request)),
    }.ok_or_else(|| ApiError::conflict("checkout_quote_not_eligible", format!("customer plan {} is not eligible for {:?} checkout",request.customer_plan_id,request.checkout_kind)))?;
    let base_amount: i64 = row.get("amount_minor");
    let base_credits: i64 = row.get("granted_credit_units");
    let quantity = if request.checkout_kind == CheckoutKind::OnDemand {
        request.quantity.unwrap_or(1)
    } else {
        1
    };
    if !(1..=10_000).contains(&quantity) {
        return Err(ApiError::unprocessable(
            "invalid_credit_quantity",
            format!("credit quantity {quantity} must be between 1 and 10000"),
        ));
    }
    let amount = base_amount.checked_mul(quantity).ok_or_else(|| {
        ApiError::unprocessable(
            "credit_quantity_overflow",
            format!("amount {base_amount} multiplied by quantity {quantity} overflows"),
        )
    })?;
    let credits = base_credits.checked_mul(quantity).ok_or_else(|| {
        ApiError::unprocessable(
            "credit_quantity_overflow",
            format!("credits {base_credits} multiplied by quantity {quantity} overflows"),
        )
    })?;
    Ok((
        amount,
        request.checkout_kind == CheckoutKind::Initial,
        row.get("currency"),
        credits,
    ))
}

async fn validate_coupon_for_quote(
    pool: &sqlx::PgPool,
    coupon: &sqlx::postgres::PgRow,
    account_id: Uuid,
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
    let local: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM coupon_checkout_reservations WHERE coupon_id=$1 AND account_id=$2 AND status IN ('COMPLETED','RESERVED')")
        .bind(id).bind(account_id).fetch_one(pool).await?;
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
        .get::<Option<i64>, _>("max_uses_per_account")
        .is_some_and(|limit| local >= limit)
    {
        return Err(ApiError::conflict(
            "coupon_usage_limit_reached",
            format!("coupon {id} account usage limit reached"),
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
    account_id: Uuid,
    key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("checkout:{account_id}:{key}"))
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn find_by_key(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    key: &str,
) -> Result<Option<CheckoutRecord>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT * FROM billing_checkouts WHERE account_id=$1 AND idempotency_key=$2 FOR UPDATE",
    )
    .bind(account_id)
    .bind(key)
    .fetch_optional(&mut **transaction)
    .await?
    .as_ref()
    .map(checkout_from_row))
}

async fn insert_checkout(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    key: &str,
    request: &CreateCheckoutRequest,
    request_hash: &str,
) -> ApiResult<CheckoutRecord> {
    let row = sqlx::query(
        "INSERT INTO billing_checkouts (checkout_id,account_id,customer_plan_id,checkout_kind, \
             on_demand_plan_id,target_plan_version_id,transaction_id,idempotency_key,request_sha256,coupon_code,payment_method_binding_id,save_payment_method, \
         credit_quantity,success_url,cancel_url) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(account_id)
    .bind(request.customer_plan_id)
    .bind(checkout_kind_text(request.checkout_kind))
    .bind(request.on_demand_plan_id)
    .bind(request.target_plan_version_id)
    .bind(&request.transaction_id)
    .bind(key)
    .bind(request_hash)
    .bind(request.coupon_code.as_deref())
    .bind(request.payment_method_binding_id)
    .bind(request.save_payment_method)
    .bind(request.quantity.unwrap_or(1))
    .bind(request.success_url.as_deref())
    .bind(request.cancel_url.as_deref())
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
        account_id: row.get("account_id"),
        customer_plan_id: row.get("customer_plan_id"),
        checkout_kind: match row.get::<String, _>("checkout_kind").as_str() {
            "INITIAL" => CheckoutKind::Initial,
            "ON_DEMAND" => CheckoutKind::OnDemand,
            _ => CheckoutKind::PlanUpgrade,
        },
        on_demand_plan_id: row.get("on_demand_plan_id"),
        target_plan_version_id: row.get("target_plan_version_id"),
        transaction_id: row.get("transaction_id"),
        idempotency_key: row.get("idempotency_key"),
        request_sha256: row.get("request_sha256"),
        collection_request_id: row.get("collection_request_id"),
        coupon_code: row.get("coupon_code"),
        payment_method_binding_id: row.get("payment_method_binding_id"),
        save_payment_method: row.get("save_payment_method"),
        completed_without_payment: row.get("completed_without_payment"),
        base_amount_minor: row.get("base_amount_minor"),
        discount_amount_minor: row.get("discount_amount_minor"),
        checkout_currency: row.get("checkout_currency"),
        quantity: row.get("credit_quantity"),
        success_url: row.get("success_url"),
        cancel_url: row.get("cancel_url"),
        created_at: row.get("created_at"),
    }
}

fn checkout_kind_text(kind: CheckoutKind) -> &'static str {
    match kind {
        CheckoutKind::Initial => "INITIAL",
        CheckoutKind::OnDemand => "ON_DEMAND",
        CheckoutKind::PlanUpgrade => "PLAN_UPGRADE",
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
