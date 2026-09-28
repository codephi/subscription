use std::time::{Duration, Instant};

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};

/// Log the completed HTTP response as ECS fields; e.g. `from_fn(log_http_request)`.
pub(super) async fn log_http_request(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let response = next.run(request).await;
    record_http_response(&method, &path, response.status(), started.elapsed());
    response
}

fn record_http_response(method: &str, path: &str, status: StatusCode, elapsed: Duration) {
    let duration = i64::try_from(elapsed.as_nanos()).unwrap_or(i64::MAX);
    tracing::info!(
        http.request.method = method,
        url.path = path,
        http.response.status_code = status.as_u16(),
        event.duration = duration,
        event.outcome = response_outcome(status),
        event.action = "http_request",
        "HTTP request completed"
    );
}

fn response_outcome(status: StatusCode) -> &'static str {
    if status.is_server_error() {
        return "failure";
    }
    "success"
}

#[cfg(test)]
mod tests {
    use super::response_outcome;
    use axum::http::StatusCode;

    #[test]
    fn response_outcome_uses_server_perspective() {
        for status in [200, 302, 400, 404, 413] {
            assert_eq!(
                response_outcome(StatusCode::from_u16(status).unwrap()),
                "success"
            );
        }
        for status in [500, 502, 503] {
            assert_eq!(
                response_outcome(StatusCode::from_u16(status).unwrap()),
                "failure"
            );
        }
    }
}
