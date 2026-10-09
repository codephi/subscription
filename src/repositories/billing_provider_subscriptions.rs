use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
    pub async fn provider_subscription_connection(
        &self,
        provider_subscription_id: &str,
    ) -> ApiResult<Option<Uuid>> {
        Ok(sqlx::query_scalar(
            "SELECT billing_connection_id FROM provider_managed_subscriptions WHERE provider='STRIPE' \
             AND provider_subscription_reference=$1",
        )
        .bind(provider_subscription_id)
        .fetch_optional(&self.pool())
        .await?)
    }

    pub async fn record_provider_subscription(
        &self,
        checkout_id: Uuid,
        provider_subscription_id: &str,
        provider_customer_id: &str,
        provider_payment_method_id: &str,
        period_start: Option<DateTime<Utc>>,
        period_end: Option<DateTime<Utc>>,
    ) -> ApiResult<()> {
        let mut transaction = self.pool().begin().await?;
        let checkout = sqlx::query(
            "SELECT c.account_id,c.customer_plan_id,hs.billing_connection_id, \
             hs.payment_method_binding_id FROM billing_checkouts c \
             JOIN billing_hosted_payment_sessions hs USING(checkout_id) WHERE c.checkout_id=$1 \
             FOR UPDATE OF c,hs",
        )
        .bind(checkout_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(
                "hosted_checkout_not_found",
                format!("subscription checkout {checkout_id} is not registered"),
            )
        })?;
        let account_id: Uuid = checkout.get("account_id");
        let customer_plan_id: Uuid = checkout.get("customer_plan_id");
        let connection_id: Uuid = checkout.get("billing_connection_id");
        let binding_id: Uuid = checkout.get("payment_method_binding_id");
        let method = sqlx::query(
            "UPDATE payment_method_bindings SET provider_payment_method_reference=$2, \
             status='ACTIVE',updated_at=clock_timestamp() WHERE payment_method_binding_id=$1",
        )
        .bind(binding_id)
        .bind(provider_payment_method_id)
        .execute(&mut *transaction)
        .await?;
        if method.rows_affected() != 1 {
            return Err(ApiError::not_found(
                "payment_method_binding_not_found",
                format!("subscription checkout {checkout_id} has no payment method binding"),
            ));
        }
        sqlx::query(
            "INSERT INTO provider_managed_subscriptions (provider_managed_subscription_id,account_id, \
             customer_plan_id,billing_connection_id,payment_method_binding_id,provider, \
             provider_subscription_reference,provider_customer_reference,status,current_period_start,current_period_end) \
             VALUES ($1,$2,$3,$4,$5,'STRIPE',$6,$7,'ACTIVE',$8,$9) \
             ON CONFLICT (provider,provider_subscription_reference) DO UPDATE SET \
             status='ACTIVE',current_period_start=EXCLUDED.current_period_start, \
             current_period_end=EXCLUDED.current_period_end,updated_at=clock_timestamp()",
        )
        .bind(Uuid::new_v4())
        .bind(account_id)
        .bind(customer_plan_id)
        .bind(connection_id)
        .bind(binding_id)
        .bind(provider_subscription_id)
        .bind(provider_customer_id)
        .bind(period_start)
        .bind(period_end)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn create_provider_renewal_collection(
        &self,
        provider_subscription_id: &str,
        invoice_id: &str,
        provider_payment_id: &str,
        amount_minor: i64,
        currency: &str,
        occurred_at: DateTime<Utc>,
    ) -> ApiResult<Uuid> {
        let mut transaction = self.pool().begin().await?;
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT collection_request_id FROM collection_requests WHERE transaction_id=$1",
        )
        .bind(format!("stripe-invoice:{invoice_id}"))
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(collection_id) = existing {
            transaction.commit().await?;
            return Ok(collection_id);
        }
        let terms = sqlx::query(
            "SELECT pms.account_id,pms.customer_plan_id,pms.billing_connection_id, \
             pms.payment_method_binding_id,cp.plan_version_id,p.price_amount_minor,p.currency, \
             p.granted_credit_units,cp.activation_status FROM provider_managed_subscriptions pms \
             JOIN customer_plans cp USING(customer_plan_id) \
             JOIN subscription_plan_versions p ON p.plan_version_id=cp.plan_version_id \
             JOIN subscriptions s USING(subscription_id) WHERE pms.provider='STRIPE' \
             AND pms.provider_subscription_reference=$1 AND pms.status='ACTIVE' FOR UPDATE OF pms,cp",
        )
        .bind(provider_subscription_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| ApiError::not_found(
            "provider_subscription_not_found",
            format!("Stripe subscription {provider_subscription_id} is not linked"),
        ))?;
        let expected_amount: i64 = terms.get("price_amount_minor");
        let expected_currency: String = terms.get("currency");
        if amount_minor != expected_amount || currency != expected_currency {
            return Err(ApiError::conflict(
                "provider_invoice_terms_mismatch",
                format!(
                    "paid invoice {invoice_id} amount and currency do not match the active plan"
                ),
            ));
        }
        let account_id: Uuid = terms.get("account_id");
        let customer_plan_id: Uuid = terms.get("customer_plan_id");
        let collection_id = Uuid::new_v4();
        let attempt_id = Uuid::new_v4();
        let now = occurred_at;
        let expires_at = now + chrono::Duration::hours(24);
        let transaction_id = format!("stripe-invoice:{invoice_id}");
        let idempotency_key = transaction_id.clone();
        let correlation_id = Uuid::new_v4();
        let request_kind =
            if terms.get::<String, _>("activation_status") == "PENDING_INITIAL_PAYMENT" {
                "INITIAL"
            } else {
                "RENEWAL"
            };
        sqlx::query(
            "INSERT INTO collection_requests (collection_request_id,account_id,customer_id,customer_plan_id, \
             plan_version_id,payment_method_binding_id,request_kind,amount_minor,currency,granted_credit_units, \
             status,transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
             VALUES ($1,$2,$2,$3,$4,$5,$14,$6,$7,$8,'PENDING_PAYMENT',$9,$10,$11,$12,$13)",
        )
        .bind(collection_id).bind(account_id).bind(customer_plan_id).bind(terms.get::<Uuid, _>("plan_version_id"))
        .bind(terms.get::<Uuid, _>("payment_method_binding_id")).bind(amount_minor).bind(currency)
        .bind(terms.get::<i64, _>("granted_credit_units")).bind(&transaction_id).bind(&idempotency_key)
        .bind(correlation_id).bind(now).bind(expires_at)
        .bind(request_kind)
        .execute(&mut *transaction).await?;
        let operation_key = format!("stripe-invoice:{invoice_id}");
        sqlx::query(
            "INSERT INTO collection_attempts (collection_attempt_id,collection_request_id,attempt_number,connector, \
             payment_method,provider_idempotency_key,status,scheduled_at,started_at,finished_at) \
             VALUES ($1,$2,1,'STRIPE','CARD',$3,'SUCCEEDED',$4,$4,$4)",
        )
        .bind(attempt_id).bind(collection_id).bind(&operation_key).bind(now)
        .execute(&mut *transaction).await?;
        sqlx::query(
            "INSERT INTO billing_payments (billing_payment_id,collection_request_id,collection_attempt_id,provider, \
             provider_payment_id,state,amount_minor,currency,confirmed_at) VALUES ($1,$2,$3,'STRIPE',$4,'PENDING',$5,$6,NULL)",
        )
        .bind(Uuid::new_v4()).bind(collection_id).bind(attempt_id).bind(provider_payment_id)
        .bind(amount_minor).bind(currency)
        .execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(collection_id)
    }

    pub async fn mark_provider_subscription_status(
        &self,
        provider_subscription_id: &str,
        status: &str,
        period_start: Option<DateTime<Utc>>,
        period_end: Option<DateTime<Utc>>,
    ) -> ApiResult<()> {
        let updated = sqlx::query(
            "UPDATE provider_managed_subscriptions SET status=$2, \
             current_period_start=COALESCE($3,current_period_start), \
             current_period_end=COALESCE($4,current_period_end),updated_at=clock_timestamp() \
             WHERE provider='STRIPE' AND provider_subscription_reference=$1",
        )
        .bind(provider_subscription_id)
        .bind(status)
        .bind(period_start)
        .bind(period_end)
        .execute(&self.pool())
        .await?;
        if updated.rows_affected() == 1 {
            return Ok(());
        }
        Err(ApiError::not_found(
            "provider_subscription_not_found",
            format!("Stripe subscription {provider_subscription_id} is not linked"),
        ))
    }
}
