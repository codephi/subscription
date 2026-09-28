use std::time::Duration;

use anyhow::{anyhow, Result};
use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_otlp::{
    HttpExporterBuilderSet, Protocol, SpanExporter, SpanExporterBuilder, WithExportConfig,
    WithHttpConfig, OTEL_EXPORTER_OTLP_TIMEOUT_DEFAULT,
};
use opentelemetry_sdk::{
    propagation::TraceContextPropagator, resource::Resource, trace::SdkTracerProvider,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub(crate) mod ecs;
mod ecs_fields;

pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(tracer_provider) = self.tracer_provider.as_ref() {
            let _ = tracer_provider.shutdown();
        }
    }
}

impl TelemetryGuard {
    pub fn force_flush(&self) -> Result<()> {
        if let Some(tracer_provider) = self.tracer_provider.as_ref() {
            tracer_provider.force_flush().map_err(Into::into)
        } else {
            Ok(())
        }
    }
}

/// Install ECS stdout logs and optional OTLP traces; e.g. `init_tracing(false)?`.
pub fn init_tracing(enabled: bool) -> Result<TelemetryGuard> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive("otel::tracing=info".parse().expect("valid directive"));
    let resource = build_resource();
    let ecs_logs = ecs::EcsLayer::new(&resource, std::io::stdout);

    let tracer_provider = enabled
        .then(|| build_tracer_provider(resource))
        .transpose()?;
    let otel_layer = tracer_provider.as_ref().map(|provider| {
        tracing_opentelemetry::layer().with_tracer(provider.tracer(env!("CARGO_PKG_NAME")))
    });
    tracing_subscriber::registry()
        .with(env_filter)
        .with(ecs_logs)
        .with(otel_layer)
        .init();
    Ok(TelemetryGuard { tracer_provider })
}

fn build_tracer_provider(resource: Resource) -> Result<SdkTracerProvider> {
    global::set_text_map_propagator(TraceContextPropagator::new());
    let use_simple = std::env::var("OTEL_USE_SIMPLE_EXPORTER")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let exporter = build_exporter(use_simple)?;

    let builder = SdkTracerProvider::builder().with_resource(resource);
    let tracer_provider = if use_simple {
        builder.with_simple_exporter(exporter).build()
    } else {
        builder.with_batch_exporter(exporter).build()
    };
    global::set_tracer_provider(tracer_provider.clone());

    Ok(tracer_provider)
}

fn build_exporter(use_simple: bool) -> Result<SpanExporter> {
    let protocol = std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL")
        .unwrap_or_else(|_| "grpc".to_string())
        .to_ascii_lowercase();

    match protocol.as_str() {
        "http/protobuf" => build_http_exporter(Protocol::HttpBinary, use_simple),
        "http/json" => build_http_exporter(Protocol::HttpJson, use_simple),
        "grpc" => SpanExporter::builder()
            .with_tonic()
            .build()
            .map_err(Into::into),
        _ => SpanExporter::builder()
            .with_tonic()
            .build()
            .map_err(Into::into),
    }
}

fn build_http_exporter(protocol: Protocol, use_simple: bool) -> Result<SpanExporter> {
    let timeout = http_export_timeout(
        std::env::var("OTEL_EXPORTER_OTLP_TRACES_TIMEOUT")
            .ok()
            .as_deref(),
        std::env::var("OTEL_EXPORTER_OTLP_TIMEOUT").ok().as_deref(),
    );
    http_exporter_builder(protocol, use_simple, timeout)?
        .build()
        .map_err(Into::into)
}

fn http_exporter_builder(
    protocol: Protocol,
    use_simple: bool,
    timeout: Duration,
) -> Result<SpanExporterBuilder<HttpExporterBuilderSet>> {
    let builder = SpanExporter::builder()
        .with_http()
        .with_protocol(protocol)
        .with_timeout(timeout);
    if use_simple {
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        return Ok(builder.with_http_client(client));
    }
    Ok(builder.with_http_client(blocking_http_client(timeout)?))
}

fn blocking_http_client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    // BatchSpanProcessor has no Tokio runtime; construct the blocking client's
    // internal runtime outside Tokio too, so initialization cannot panic.
    std::thread::spawn(move || {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
    })
    .join()
    .map_err(|_| anyhow!("OTLP HTTP client thread panicked; expected a blocking client"))?
    .map_err(Into::into)
}

fn http_export_timeout(trace_timeout: Option<&str>, global_timeout: Option<&str>) -> Duration {
    [trace_timeout, global_timeout]
        .into_iter()
        .flatten()
        .find_map(|value| value.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(OTEL_EXPORTER_OTLP_TIMEOUT_DEFAULT)
}

fn build_resource() -> Resource {
    let mut builder = Resource::builder();
    if std::env::var("OTEL_SERVICE_NAME").is_err() {
        builder = builder.with_service_name(env!("CARGO_PKG_NAME"));
    }
    builder.build()
}

#[cfg(test)]
#[path = "telemetry/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "telemetry/ecs_tests.rs"]
mod ecs_tests;

#[cfg(test)]
#[path = "telemetry/access_log_tests.rs"]
mod access_log_tests;
