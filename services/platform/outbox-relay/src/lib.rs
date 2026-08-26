//! CompanyOS outbox relay — Phase 1.8 platform service.
//!
//! Polls `outbox_event`, publishes to NATS JetStream (or logs in-memory when
//! `NATS_URL` is unset), and exposes `/healthz` + `/metrics` on `:8090`.

pub mod metrics_api;
pub mod openapi;
pub mod publisher;

use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router};
use companyos_outbox::relay::RelayMetrics;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use crate::metrics_api::HttpState;

/// Build the optional HTTP surface (`/healthz`, `/metrics`, OpenAPI).
pub fn build_router(state: HttpState) -> Router {
    let x_request_id = http::HeaderName::from_static("x-request-id");
    Router::new()
        .route(
            "/livez",
            get(|| async { Json(serde_json::json!({ "status": "ok" })) }),
        )
        .route(
            "/readyz",
            get(|| async { Json(serde_json::json!({ "status": "ready" })) }),
        )
        .route(
            "/healthz",
            get(|| async {
                Json(serde_json::json!({ "status": "ok", "service": "companyos-outbox-relay" }))
            }),
        )
        .merge(metrics_api::router())
        .merge(openapi::router())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}

/// Shared metrics handle for the relay loop + HTTP `/metrics`.
pub fn new_metrics() -> Arc<RelayMetrics> {
    Arc::new(RelayMetrics::default())
}
