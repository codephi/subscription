use anyhow::{Context, Result};

pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub subscription_api_url: String,
    pub accounts_webhook_secret: String,
    pub tasklab_web_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            host: value("APP_HOST", "127.0.0.1"),
            port: value("APP_PORT", "3001").parse()?,
            database_url: required("DATABASE_URL")?,
            subscription_api_url: required("SUBSCRIPTION_API_URL")?,
            accounts_webhook_secret: required("ACCOUNTS_WEBHOOK_SECRET")?,
            tasklab_web_url: value("TASKLAB_WEB_URL", "http://127.0.0.1:5174"),
        })
    }
}

fn value(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn required(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("configure {name} in example/.env"))
}
