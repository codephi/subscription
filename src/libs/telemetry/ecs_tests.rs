use std::{
    io,
    sync::{Arc, Mutex},
};

use opentelemetry::{trace::TracerProvider, KeyValue};
use opentelemetry_sdk::{trace::SdkTracerProvider, Resource};
use serde_json::Value;
use tracing::Dispatch;
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt, EnvFilter};

use super::ecs::EcsLayer;

#[derive(Clone, Default)]
pub(super) struct FakeLogWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for FakeLogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for FakeLogWriter {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

impl FakeLogWriter {
    pub(super) fn events(&self) -> Vec<Value> {
        let bytes = self.0.lock().unwrap();
        let output = std::str::from_utf8(&bytes).unwrap();
        assert!(output.ends_with('\n'));
        assert!(!output.contains('\u{1b}'));
        output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

pub(super) fn ecs_dispatch(writer: FakeLogWriter) -> Dispatch {
    let resource = Resource::builder_empty()
        .with_service_name("subscription-tests")
        .build();
    Dispatch::new(tracing_subscriber::registry().with(EcsLayer::new(&resource, writer)))
}

pub(super) fn traced_dispatch(writer: FakeLogWriter, provider: &SdkTracerProvider) -> Dispatch {
    Dispatch::new(
        tracing_subscriber::registry()
            .with(EcsLayer::new(&Resource::builder_empty().build(), writer))
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("ecs-tests"))),
    )
}

#[test]
fn ecs_events_are_single_line_json_with_typed_fields_and_escaped_messages() {
    let writer = FakeLogWriter::default();
    tracing::dispatcher::with_default(&ecs_dispatch(writer.clone()), || {
        tracing::warn!(attempt = 3_u64, offset = -2_i64, retry = true, ratio = 0.5_f64,
            detail = ?vec![1, 2], "message with \"quotes\"\nnext line");
    });
    let events = writer.events();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event["message"], "message with \"quotes\"\nnext line");
    assert_eq!(event["log"]["level"], "warn");
    assert_eq!(event["ecs"]["version"], "9.5.0");
    assert_eq!(
        event["labels"],
        serde_json::json!({"attempt": 3, "offset": -2,
        "retry": true, "ratio": 0.5, "detail": "[1, 2]"})
    );
    chrono::DateTime::parse_from_rfc3339(event["@timestamp"].as_str().unwrap()).unwrap();
    assert_eq!(event["log"]["logger"], module_path!());
    assert_eq!(event["service"]["name"], "subscription-tests");
    assert!(event.get("trace").is_none());
}

#[test]
fn ecs_service_identity_uses_otel_resource_attributes() {
    let writer = FakeLogWriter::default();
    let resource = Resource::builder_empty()
        .with_service_name("billing-api")
        .with_attributes([
            KeyValue::new("service.version", "2.1"),
            KeyValue::new("deployment.environment.name", "staging"),
            KeyValue::new("deployment.environment", "legacy"),
        ])
        .build();
    let subscriber = tracing_subscriber::registry().with(EcsLayer::new(&resource, writer.clone()));
    tracing::subscriber::with_default(subscriber, || tracing::info!("started"));
    assert_eq!(
        writer.events()[0]["service"],
        serde_json::json!({
            "name": "billing-api", "version": "2.1", "environment": "staging"
        })
    );
}

#[test]
fn ecs_service_defaults_and_legacy_environment_are_supported() {
    let writer = FakeLogWriter::default();
    let resource = Resource::builder_empty()
        .with_attribute(KeyValue::new("deployment.environment", "local"))
        .build();
    let subscriber = tracing_subscriber::registry().with(EcsLayer::new(&resource, writer.clone()));
    tracing::subscriber::with_default(subscriber, || tracing::info!("started"));
    assert_eq!(
        writer.events()[0]["service"],
        serde_json::json!({
            "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION"), "environment": "local"
        })
    );
}

#[test]
fn ecs_fields_follow_span_updates_and_event_precedence() {
    let writer = FakeLogWriter::default();
    tracing::dispatcher::with_default(&ecs_dispatch(writer.clone()), || {
        let parent = tracing::info_span!("parent", account_id = "account-1", attempt = 1_u64);
        parent.record("attempt", 2_u64);
        parent.in_scope(|| {
            let child = tracing::info_span!("child", job_id = "job-1");
            child.in_scope(|| tracing::info!(attempt = 3_u64, "processed"));
            tracing::info!("parent event");
        });
    });
    let events = writer.events();
    assert_eq!(
        events[0]["labels"],
        serde_json::json!({
            "account_id": "account-1", "job_id": "job-1", "attempt": 3
        })
    );
    assert_eq!(events[1]["labels"]["attempt"], 2);
    assert!(events[1]["labels"].get("job_id").is_none());
}

#[test]
fn ecs_maps_existing_error_fields_and_keeps_required_metadata() {
    let writer = FakeLogWriter::default();
    tracing::dispatcher::with_default(&ecs_dispatch(writer.clone()), || {
        let error = io::Error::other("database unavailable");
        tracing::error!(
            error = &error as &dyn std::error::Error,
            error_code = "db_unavailable",
            log.level = "spoof",
            ecs.version = "invalid",
            "job failed"
        );
        tracing::error!(error = %error, "display error");
    });
    let events = writer.events();
    assert_eq!(events[0]["error"]["message"], "database unavailable");
    assert_eq!(events[0]["error"]["code"], "db_unavailable");
    assert_eq!(events[0]["log"]["level"], "error");
    assert_eq!(events[0]["ecs"]["version"], "9.5.0");
    assert_eq!(events[1]["error"]["message"], "database unavailable");
}

#[test]
fn ecs_uses_explicit_event_parent_for_fields_and_trace_ids() {
    let writer = FakeLogWriter::default();
    let provider = SdkTracerProvider::builder().build();
    tracing::dispatcher::with_default(&traced_dispatch(writer.clone(), &provider), || {
        let parent = tracing::info_span!("parent", operation = "explicit");
        parent.in_scope(|| tracing::info!("parent event"));
        let other = tracing::info_span!("other", operation = "ambient");
        other.in_scope(|| tracing::info!(parent: &parent, "explicit parent event"));
        tracing::info!(parent: None, "no parent");
    });
    let events = writer.events();
    assert_eq!(events[1]["labels"]["operation"], "explicit");
    assert_eq!(events[0]["trace"], events[1]["trace"]);
    assert_eq!(events[0]["span"], events[1]["span"]);
    assert_eq!(events[0]["trace"]["id"].as_str().unwrap().len(), 32);
    assert_eq!(events[0]["span"]["id"].as_str().unwrap().len(), 16);
    assert!(events[2].get("trace").is_none());
}

#[test]
fn ecs_respects_subscriber_level_filtering() {
    let writer = FakeLogWriter::default();
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new("warn"))
        .with(EcsLayer::new(
            &Resource::builder_empty().build(),
            writer.clone(),
        ));
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("filtered");
        tracing::warn!("kept");
    });
    let events = writer.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["message"], "kept");
}
