use std::{io::Write, sync::OnceLock};

use chrono::{SecondsFormat, Utc};
use opentelemetry::{trace::TraceContextExt, Key};
use opentelemetry_sdk::Resource;
use serde_json::{json, Value};
use tracing::{dispatcher::WeakDispatch, span, Dispatch, Event, Metadata, Subscriber};
use tracing_subscriber::{fmt::MakeWriter, layer::Context, registry::LookupSpan, Layer};

use super::ecs_fields::{apply_ecs_fields, EcsFields};

pub(crate) struct EcsLayer<W> {
    service: Value,
    writer: W,
    dispatch: OnceLock<WeakDispatch>,
}

impl<W> EcsLayer<W> {
    /// Configure ECS output using the same service identity as traces, e.g. `new(&resource, stdout)`.
    pub(crate) fn new(resource: &Resource, writer: W) -> Self {
        Self {
            service: service_identity(resource),
            writer,
            dispatch: OnceLock::new(),
        }
    }

    fn event_document(&self, metadata: &Metadata<'_>) -> Value {
        json!({
            "@timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            "ecs": { "version": "9.5.0" },
            "log": { "level": metadata.level().as_str().to_ascii_lowercase(), "logger": metadata.target() },
            "service": self.service,
            "message": metadata.name(),
            "event": { "kind": "event" },
        })
    }
}

impl<W: for<'writer> MakeWriter<'writer>> EcsLayer<W> {
    fn write_document(&self, document: &Value, metadata: &Metadata<'_>) {
        if let Ok(mut line) = serde_json::to_vec(document) {
            line.push(b'\n');
            let _ = self.writer.make_writer_for(metadata).write_all(&line);
        }
    }
}

fn service_identity(resource: &Resource) -> Value {
    let mut service = json!({
        "name": resource.get(&Key::from_static_str("service.name"))
            .map(|value| value.to_string()).unwrap_or_else(|| env!("CARGO_PKG_NAME").into()),
        "version": resource.get(&Key::from_static_str("service.version"))
            .map(|value| value.to_string()).unwrap_or_else(|| env!("CARGO_PKG_VERSION").into()),
    });
    let environment = resource
        .get(&Key::from_static_str("deployment.environment.name"))
        .or_else(|| resource.get(&Key::from_static_str("deployment.environment")));
    if let Some(environment) = environment {
        service["environment"] = environment.to_string().into();
    }
    service
}

impl<S, W> Layer<S> for EcsLayer<W>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    fn on_register_dispatch(&self, dispatch: &Dispatch) {
        let _ = self.dispatch.set(dispatch.downgrade());
    }

    fn on_new_span(
        &self,
        attributes: &span::Attributes<'_>,
        id: &span::Id,
        context: Context<'_, S>,
    ) {
        let mut fields = EcsFields::default();
        attributes.record(&mut fields);
        if let Some(span) = context.span(id) {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, context: Context<'_, S>) {
        let Some(span) = context.span(id) else { return };
        let mut extensions = span.extensions_mut();
        if let Some(fields) = extensions.get_mut::<EcsFields>() {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        let mut document = self.event_document(event.metadata());
        let mut fields = inherited_fields(event, &context);
        event.record(&mut fields);
        apply_ecs_fields(&mut document, fields);
        if let (Some(span), Some(dispatch)) = (
            context.event_span(event),
            self.dispatch.get().and_then(WeakDispatch::upgrade),
        ) {
            apply_trace_context(&mut document, &span.id(), &dispatch);
        }
        self.write_document(&document, event.metadata());
    }
}

fn inherited_fields<S>(event: &Event<'_>, context: &Context<'_, S>) -> EcsFields
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    let mut fields = EcsFields::default();
    let Some(scope) = context.event_scope(event) else {
        return fields;
    };
    for span in scope.from_root() {
        if let Some(parent_fields) = span.extensions().get::<EcsFields>() {
            fields.0.extend(parent_fields.0.clone());
        }
    }
    fields
}

fn apply_trace_context(document: &mut Value, span_id: &span::Id, dispatch: &Dispatch) {
    let context = tracing_opentelemetry::get_otel_context(span_id, dispatch);
    let Some(context) = context else { return };
    let span = context.span();
    let span_context = span.span_context();
    if span_context.is_valid() {
        document["trace"]["id"] = span_context.trace_id().to_string().into();
        document["span"]["id"] = span_context.span_id().to_string().into();
    }
}
