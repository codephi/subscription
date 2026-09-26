use std::sync::Arc;

use crate::repositories::database::DatabaseRepository;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<SharedState>,
}

#[derive(Clone)]
struct SharedState {
    pub database: DatabaseRepository,
    pub accounts_webhook_secret: Option<String>,
    pub public_api_base_url: Option<String>,
}

impl AppState {
    pub fn new(database: DatabaseRepository) -> Self {
        let inner = SharedState {
            database,
            accounts_webhook_secret: None,
            public_api_base_url: None,
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    pub fn database(&self) -> DatabaseRepository {
        self.inner.database.clone()
    }

    pub fn with_accounts_webhook_secret(mut self, secret: Option<String>) -> Self {
        Arc::make_mut(&mut self.inner).accounts_webhook_secret = secret;
        self
    }

    pub fn accounts_webhook_secret(&self) -> Option<String> {
        self.inner.accounts_webhook_secret.clone()
    }

    pub fn with_public_api_base_url(mut self, base_url: Option<String>) -> Self {
        Arc::make_mut(&mut self.inner).public_api_base_url = base_url;
        self
    }

    pub fn public_api_base_url(&self) -> Option<String> {
        self.inner.public_api_base_url.clone()
    }
}

#[cfg(test)]
mod tests {
    use sqlx::postgres::PgPoolOptions;

    use super::AppState;
    use crate::repositories::database::DatabaseRepository;

    #[tokio::test]
    async fn state_exposes_database_repository() {
        let pool = PgPoolOptions::new().connect_lazy("postgres://postgres:postgres@localhost/test");
        let repository = DatabaseRepository::new(pool.expect("lazy pool"));
        let state = AppState::new(repository.clone());

        let _ = state.database();
    }
}
