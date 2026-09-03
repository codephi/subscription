use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use axum::{response::Response, Router};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use subscription::{
    config::{
        otel_enabled_from_env, AppConfig, CorsConfig, McpConfig, DEFAULT_BODY_LIMIT_BYTES,
        DEFAULT_MCP_PATH,
    },
    db::{init_pool, run_migrations},
    libs::telemetry,
    repositories::database::DatabaseRepository,
    routes::create_router,
    state::AppState,
};
use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};
use tokio::sync::OnceCell;

static MIGRATIONS: OnceCell<()> = OnceCell::const_new();
static TEST_DB_URL: OnceCell<String> = OnceCell::const_new();
static TELEMETRY_GUARD: OnceCell<telemetry::TelemetryGuard> = OnceCell::const_new();
static OTEL_ENDPOINT: OnceCell<String> = OnceCell::const_new();
static ENV_LOADED: OnceCell<()> = OnceCell::const_new();

#[allow(dead_code)]
pub async fn setup_router() -> Router {
    setup_router_with_mcp(false).await
}

#[allow(dead_code)]
pub async fn setup_router_with_mcp(mcp_enabled: bool) -> Router {
    setup_router_with_options(mcp_enabled, None).await.0
}

pub async fn setup_router_with_options(
    mcp_enabled: bool,
    accounts_webhook_secret: Option<String>,
) -> (Router, PgPool) {
    let database_url = database_url().await;
    init_telemetry().await;
    let pool = init_pool_with_retry(&database_url).await;
    run_migrations_once(pool.clone()).await;
    let state = AppState::new(DatabaseRepository::new(pool.clone()))
        .with_accounts_webhook_secret(accounts_webhook_secret.clone());
    let config = test_config(database_url, mcp_enabled, accounts_webhook_secret);
    (create_router(state, &config), pool)
}

pub async fn response_json(response: Response) -> Value {
    let body = response_bytes(response).await;
    serde_json::from_slice(&body).expect("failed to parse json response")
}

pub async fn response_bytes(response: Response) -> bytes::Bytes {
    response
        .into_body()
        .collect()
        .await
        .expect("failed to read response body")
        .to_bytes()
}

#[allow(dead_code)]
pub async fn flush_telemetry() {
    if let Some(guard) = TELEMETRY_GUARD.get() {
        guard.force_flush().expect("failed to flush telemetry");
    }
}

fn test_config(
    database_url: String,
    mcp_enabled: bool,
    accounts_webhook_secret: Option<String>,
) -> AppConfig {
    AppConfig {
        database_url,
        host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        cors: CorsConfig::Permissive,
        body_limit_bytes: DEFAULT_BODY_LIMIT_BYTES,
        otel_enabled: otel_enabled_from_env(),
        mcp: McpConfig {
            enabled: mcp_enabled,
            path: DEFAULT_MCP_PATH.to_string(),
            cors: CorsConfig::Permissive,
        },
        accounts_webhook_secret,
        outbound_event_webhook: None,
    }
}

async fn run_migrations_once(pool: PgPool) {
    MIGRATIONS
        .get_or_init(|| async move {
            run_migrations(&pool)
                .await
                .expect("failed to run database migrations");
        })
        .await;
}

async fn database_url() -> String {
    TEST_DB_URL.get_or_init(start_postgres).await.clone()
}

async fn start_postgres() -> String {
    let image = GenericImage::new("pgvector/pgvector", "pg18")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_DB", "postgres");
    let container = image
        .start()
        .await
        .expect("failed to start postgres container");
    let container = Box::leak(Box::new(container));
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to resolve postgres mapped port");
    format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres")
}

async fn init_telemetry() {
    load_env().await;
    if !otel_enabled_from_env() {
        TELEMETRY_GUARD
            .get_or_init(|| async { telemetry::init_tracing(false).expect("init tracing") })
            .await;
        return;
    }
    configure_test_otel().await;
    TELEMETRY_GUARD
        .get_or_init(|| async { telemetry::init_tracing(true).expect("init tracing") })
        .await;
}

async fn configure_test_otel() {
    if let Some(endpoint) = otel_endpoint().await {
        set_env_if_missing("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");
        set_env_if_missing("OTEL_EXPORTER_OTLP_ENDPOINT", &endpoint);
    }
    set_env_if_missing("OTEL_EXPORTER_OTLP_TIMEOUT", "2000");
    set_env_if_missing("OTEL_EXPORTER_OTLP_TRACES_TIMEOUT", "2000");
    set_env_if_missing("OTEL_TRACES_SAMPLER", "always_on");
    set_env_if_missing("OTEL_USE_SIMPLE_EXPORTER", "true");
    set_env_if_missing("OTEL_BSP_SCHEDULE_DELAY", "200");
    set_env_if_missing("OTEL_SERVICE_NAME", "subscription-tests");
}

async fn otel_endpoint() -> Option<String> {
    if external_otel_is_configured() {
        return None;
    }
    Some(OTEL_ENDPOINT.get_or_init(start_jaeger).await.clone())
}

fn external_otel_is_configured() -> bool {
    std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").is_ok()
        || std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
}

async fn start_jaeger() -> String {
    let image = GenericImage::new("jaegertracing/jaeger", "latest")
        .with_exposed_port(4317.tcp())
        .with_wait_for(WaitFor::seconds(3))
        .with_env_var("COLLECTOR_OTLP_ENABLED", "true")
        .with_env_var("COLLECTOR_OTLP_GRPC_HOST_PORT", "0.0.0.0:4317");
    let container = image
        .start()
        .await
        .expect("failed to start jaeger container");
    let container = Box::leak(Box::new(container));
    let port = container
        .get_host_port_ipv4(4317)
        .await
        .expect("failed to resolve jaeger mapped port");
    format!("http://127.0.0.1:{port}")
}

async fn load_env() {
    ENV_LOADED
        .get_or_init(|| async {
            dotenvy::dotenv().ok();
        })
        .await;
}

fn set_env_if_missing(key: &str, value: &str) {
    if std::env::var(key).is_err() {
        std::env::set_var(key, value);
    }
}

async fn init_pool_with_retry(database_url: &str) -> PgPool {
    for attempt in 1..=10 {
        match init_pool(database_url).await {
            Ok(pool) => return pool,
            Err(error) if attempt == 10 => panic!("failed to initialize pool: {error}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    unreachable!("retry loop always returns or panics")
}
