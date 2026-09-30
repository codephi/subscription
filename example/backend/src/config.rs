use anyhow::{Context, Result};

pub struct Config {
    pub host: String,
    pub port: u16,
    pub app_public_url: String,
    pub database_url: String,
    pub subscription_api_url: String,
    pub accounts_webhook_secret: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            host: value("APP_HOST", "127.0.0.1"),
            port: value("APP_PORT", "3001").parse()?,
            app_public_url: public_app_url(value("APP_PUBLIC_URL", "http://localhost:5174"))?,
            database_url: required("DATABASE_URL")?,
            subscription_api_url: required("SUBSCRIPTION_API_URL")?,
            accounts_webhook_secret: required("ACCOUNTS_WEBHOOK_SECRET")?,
        })
    }
}

fn public_app_url(value: String) -> Result<String> {
    let url = url::Url::parse(&value)
        .with_context(|| format!("APP_PUBLIC_URL {value:?} must be a valid origin"))?;
    let local_http = url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
    let secure_https = url.scheme() == "https";
    let valid_origin = (local_http || secure_https)
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    anyhow::ensure!(
        valid_origin,
        "APP_PUBLIC_URL {value:?} must be an HTTPS origin, or a loopback HTTP origin for local development"
    );
    Ok(url.origin().ascii_serialization())
}

fn value(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn required(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("configure {name} in example/.env"))
}

#[cfg(test)]
mod tests {
    use super::public_app_url;

    #[test]
    fn accepts_production_https_and_local_loopback_origins() {
        assert_eq!(
            public_app_url("https://tasklab.example/".into()).unwrap(),
            "https://tasklab.example"
        );
        assert_eq!(
            public_app_url("http://localhost:5174".into()).unwrap(),
            "http://localhost:5174"
        );
    }

    #[test]
    fn rejects_insecure_remote_and_non_origin_urls() {
        for value in [
            "http://tasklab.example",
            "https://tasklab.example/app",
            "https://tasklab.example/?next=/",
        ] {
            assert!(public_app_url(value.into()).is_err(), "accepted {value}");
        }
    }
}
