//! Minimal OpenAPI for outbox-relay HTTP surface.

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::metrics_api::{HttpState, MetricsResponse};

#[derive(OpenApi)]
#[openapi(
    paths(crate::metrics_api::metrics),
    components(schemas(MetricsResponse)),
    tags((name = "outbox-relay", description = "Outbox → NATS JetStream relay")),
    info(
        title = "CompanyOS Outbox Relay",
        version = "0.1.0",
        description = "Phase 1.8 — transactional outbox publisher health and lag metrics."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<HttpState> {
    Router::new().route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
