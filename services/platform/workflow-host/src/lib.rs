//! CompanyOS workflow host — Phase 1.8 Temporal catalogue.

pub mod activities;
pub mod catalogue;
pub mod openapi;

use axum::routing::get;
use axum::{Json, Router};
use tower_http::trace::TraceLayer;

/// Temporal task queue for the platform workflow host.
pub const TASK_QUEUE: &str = "companyos-platform-workflows";

/// Resolve TEMPORAL_NAMESPACE — never share across env.
/// Default `companyos-local`; CI should set `companyos-ci`.
pub fn temporal_namespace() -> String {
    std::env::var("TEMPORAL_NAMESPACE").unwrap_or_else(|_| "companyos-local".into())
}

pub fn build_router() -> Router {
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
                Json(serde_json::json!({
                    "status": "ok",
                    "service": "companyos-workflow-host",
                    "namespace": temporal_namespace(),
                    "task_queue": TASK_QUEUE,
                    "workflows": catalogue::WorkflowType::all()
                        .iter()
                        .map(|w| w.as_str())
                        .collect::<Vec<_>>(),
                }))
            }),
        )
        .merge(openapi::router())
        .layer(TraceLayer::new_for_http())
}
