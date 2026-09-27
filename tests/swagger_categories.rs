mod support;

use std::collections::BTreeSet;

use axum::{body::Body, http::Request};
use serde_json::Value;
use tower::ServiceExt;

const EXPECTED_TAGS: [&str; 10] = [
    "Billing",
    "Catalog",
    "Checkouts",
    "Credits",
    "Integrations",
    "Operations",
    "Subscriptions",
    "System",
    "Usage",
    "Wallets",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn swagger_operations_have_one_declared_domain_category() {
    let app = support::setup_router().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .expect("OpenAPI request"),
        )
        .await
        .expect("OpenAPI response");
    let document = support::response_json(response).await;
    assert_eq!(declared_tags(&document), expected_tags());
    assert_operation_tags(&document);
}

fn declared_tags(document: &Value) -> BTreeSet<&str> {
    document["tags"]
        .as_array()
        .expect("declared OpenAPI tags")
        .iter()
        .map(|tag| tag["name"].as_str().expect("tag name"))
        .collect()
}

fn expected_tags() -> BTreeSet<&'static str> {
    EXPECTED_TAGS.into_iter().collect()
}

fn assert_operation_tags(document: &Value) {
    let paths = document["paths"].as_object().expect("OpenAPI paths");
    for (path, operations) in paths {
        for (method, operation) in operations.as_object().expect("path operations") {
            assert_single_declared_tag(path, method, operation);
        }
    }
}

fn assert_single_declared_tag(path: &str, method: &str, operation: &Value) {
    let tags = operation["tags"]
        .as_array()
        .unwrap_or_else(|| panic!("OpenAPI operation {method} {path} must have a domain tag"));
    assert_eq!(tags.len(), 1, "OpenAPI operation {method} {path}");
    let tag = tags[0].as_str().expect("operation tag name");
    assert!(
        EXPECTED_TAGS.contains(&tag),
        "OpenAPI operation {method} {path} has undeclared tag {tag}"
    );
}
