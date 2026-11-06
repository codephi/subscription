use std::fmt::Debug;

use serde_json::{Map, Value};
use tracing::field::{Field, Visit};

#[derive(Clone, Default)]
pub(super) struct EcsFields(pub Map<String, Value>);

impl Visit for EcsFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.record_str(field, &format!("{value:?}"));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.record_str(field, &value.to_string());
    }
}

pub(super) fn apply_ecs_fields(document: &mut Value, fields: EcsFields) {
    for (name, value) in fields.0 {
        match name.as_str() {
            "message" => document["message"] = value,
            "error" | "error.message" => document["error"]["message"] = value,
            "error_code" | "error.code" => document["error"]["code"] = value,
            "http.request.method" => document["http"]["request"]["method"] = value,
            "http.response.status_code" => document["http"]["response"]["status_code"] = value,
            "url.path" => document["url"]["path"] = value,
            "event.duration" => document["event"]["duration"] = value,
            "event.outcome" => document["event"]["outcome"] = value,
            "event.action" => document["event"]["action"] = value,
            // OTEL span attributes can include full URLs and query strings.
            "url.full" | "url.query" | "http.target" | "http.url" => {}
            _ => document["labels"][name.replace('.', "_")] = value,
        }
    }
}
