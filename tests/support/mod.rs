use std::{
    net::{IpAddr, Ipv4Addr},
    sync::OnceLock,
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
    db::run_migrations,
    libs::telemetry,
    repositories::database::DatabaseRepository,
    routes::create_router,
    state::AppState,
};
use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};
use tokio::sync::OnceCell;

static TELEMETRY_GUARD: OnceCell<telemetry::TelemetryGuard> = OnceCell::const_new();
static OTEL_ENDPOINT: OnceCell<String> = OnceCell::const_new();
static ENV_LOADED: OnceCell<()> = OnceCell::const_new();
static DOCKER_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

struct TestPostgres {
    container: Option<ContainerAsync<GenericImage>>,
}

impl Drop for TestPostgres {
    fn drop(&mut self) {
        let container = self.container.take().expect("owned test container");
        let (completed, wait) = std::sync::mpsc::sync_channel(1);
        // Pool callbacks can be dropped while a test runtime is shutting down. Cleanup
        // must run on the independent Docker runtime, not on that stopped I/O driver.
        docker_runtime().spawn(async move {
            let _ = completed.send(container.rm().await);
        });
        wait.recv_timeout(Duration::from_secs(30))
            .expect("test container cleanup completion")
            .expect("test container removed");
    }
}

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
    let (container, port) = start_postgres().await;
    let database_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    init_telemetry().await;
    let pool = init_pool_with_retry(&database_url, container).await;
    run_migrations(&pool).await.expect("fixture migrations");
    let state = AppState::new(DatabaseRepository::new(pool.clone()))
        .with_accounts_webhook_secret(accounts_webhook_secret.clone());
    let config = test_config(database_url, mcp_enabled, accounts_webhook_secret);
    (create_router(state, &config), pool)
}

#[allow(dead_code)]
pub async fn response_json(response: Response) -> Value {
    let body = response_bytes(response).await;
    serde_json::from_slice(&body).expect("failed to parse json response")
}

#[allow(dead_code)]
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

async fn start_postgres() -> (TestPostgres, u16) {
    docker_runtime()
        .spawn(async {
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
            let port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("postgres mapped port");
            (
                TestPostgres {
                    container: Some(container),
                },
                port,
            )
        })
        .await
        .expect("start disposable postgres")
}

fn docker_runtime() -> &'static tokio::runtime::Runtime {
    // Docker's shared HTTP client must outlive individual #[tokio::test] runtimes.
    DOCKER_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Docker test runtime")
    })
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

async fn init_pool_with_retry(database_url: &str, container: TestPostgres) -> PgPool {
    // The pool owns the callback, keeping the disposable database alive until the last
    // router/repository/connection releases it, including during test failure unwinding.
    let options = sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .after_connect(move |_, _| {
            let _container_lifetime = &container;
            Box::pin(async { Ok(()) })
        });
    for attempt in 1..=10 {
        match options.clone().connect(database_url).await {
            Ok(pool) => return pool,
            Err(error) if attempt == 10 => panic!("failed to initialize pool: {error}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    unreachable!("retry loop always returns or panics")
}
