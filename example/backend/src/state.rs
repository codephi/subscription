use sqlx::SqlitePool;

use crate::api::SubscriptionClient;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub subscription: SubscriptionClient,
    pub accounts_webhook_secret: String,
    pub stripe_publishable_key: Option<String>,
}

impl AppState {
    pub fn new(
        pool: SqlitePool,
        subscription: SubscriptionClient,
        accounts_webhook_secret: String,
        stripe_publishable_key: Option<String>,
    ) -> Self {
        Self {
            pool,
            subscription,
            accounts_webhook_secret,
            stripe_publishable_key,
        }
    }
}
