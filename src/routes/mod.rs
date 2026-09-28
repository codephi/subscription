use axum::{middleware, Router};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use tower_http::limit::RequestBodyLimitLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{config::AppConfig, state::AppState};

mod access_log;
pub mod admin_queries;
pub mod admission;
pub mod audit_admin;
pub mod billing;
pub mod billing_investigation;
pub mod catalog;
pub mod catalog_admin;
pub mod checkouts;
mod cors;
pub mod credits;
pub mod inbox_admin;
pub mod integrations;
pub mod internal;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod plans;
pub mod system;
pub mod usage;
pub mod wallets;

#[derive(OpenApi)]
#[openapi(
    info(
        title = env!("CARGO_PKG_NAME"),
        description = "Subscription, credits, usage, billing, and administrative operations API.",
        version = env!("CARGO_PKG_VERSION"),
        license(name = "Apache-2.0", url = "https://www.apache.org/licenses/LICENSE-2.0.html")
    ),
    tags(
        (name = "System", description = "Liveness and diagnostic endpoints"),
        (name = "Integrations", description = "Signed inbound events from external domains"),
        (name = "Catalog", description = "Products, items, prices, and catalog scope"),
        (name = "Wallets", description = "Provisioned customer and item wallet lifecycle"),
        (name = "Credits", description = "Credit grants, balances, and customer statements"),
        (name = "Subscriptions", description = "Commercial plans, customer plans, and cycles"),
        (name = "Usage", description = "Metered usage, eligibility, and item statements"),
        (name = "Operations", description = "Administrative replay and reconciliation operations"),
        (name = "Billing", description = "Provider-neutral collection and payment workflows"),
        (name = "Checkouts", description = "Provider-neutral checkout workflows")
    )
)]
struct ApiDoc;

pub fn create_router(state: AppState, config: &AppConfig) -> Router {
    let (api_router, api) = api_router().split_for_parts();

    let api_router = api_router.merge(SwaggerUi::new("/docs").url("/openapi.json", api));

    let api_router = api_router
        .layer(cors::build_cors_layer(&config.cors, None))
        .layer(RequestBodyLimitLayer::new(config.body_limit_bytes));

    #[cfg(feature = "mcp")]
    let api_router = if config.mcp.enabled {
        api_router.merge(mcp::router(&config.mcp))
    } else {
        api_router
    };

    let api_router = api_router.layer(middleware::from_fn(access_log::log_http_request));
    let api_router = if config.otel_enabled {
        api_router
            .layer(OtelInResponseLayer)
            .layer(OtelAxumLayer::default().filter(|path| path != "/health"))
    } else {
        api_router
    };

    api_router.with_state(state)
}

/// Return the generated REST contract; e.g. `openapi_document().to_json()`.
pub fn openapi_document() -> utoipa::openapi::OpenApi {
    api_router().split_for_parts().1
}

fn api_router() -> utoipa_axum::router::OpenApiRouter<AppState> {
    utoipa_axum::router::OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(system::router())
        .merge(internal::router())
        .merge(inbox_admin::router())
        .merge(catalog::router())
        .merge(catalog_admin::router())
        .merge(credits::router())
        .merge(plans::router())
        .merge(admission::router())
        .merge(audit_admin::router())
        .merge(admin_queries::router())
        .merge(billing::router())
        .merge(checkouts::router())
        .merge(integrations::router())
        .merge(billing_investigation::router())
        .merge(usage::router())
        .merge(wallets::router())
}

#[cfg(test)]
mod tests {
    use super::openapi_document;

    #[test]
    fn administrative_reads_are_in_openapi() {
        let paths = openapi_document().paths;
        assert!(paths.paths.contains_key("/v1/admin/workspaces"));
        assert!(paths
            .paths
            .contains_key("/v1/admin/workspaces/{workspace_id}"));
        assert!(paths
            .paths
            .contains_key("/v1/admin/workspaces/{workspace_id}/terminate"));
        assert!(paths
            .paths
            .contains_key("/v1/admin/workspaces/{workspace_id}/customer-plans"));
        assert!(paths
            .paths
            .contains_key("/v1/admin/billing/stripe-defaults"));
    }
}
