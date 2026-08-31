//! CompanyOS analytics and reporting service — Phase 3.2.
//!
//! ADR-011: ClickHouse (or Postgres mirror) is fed **only** from events via
//! `/internal/ingest`. No direct reads of OLTP invoice tables for warehouse loads.

pub mod auth;
pub mod export;
pub mod forecast;
pub mod handlers;
pub mod metrics;
pub mod openapi;
pub mod principal;
pub mod query;
pub mod schedule;
pub mod state;
pub mod types;

use axum::routing::get;
use axum::{Json, Router};
use state::AppState;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

pub fn split_sql(sql: &str) -> Vec<String> {
    let cleaned: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    cleaned
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    companyos_tenancy::with_schema_migration_lock(pool, || async {
        for migration in [
            include_str!("../migrations/001_analytics.sql"),
            include_str!("../migrations/002_analytics_phase32.sql"),
        ] {
            for stmt in split_sql(migration) {
                companyos_tenancy::execute_migration_stmt(pool, &stmt).await?;
            }
        }
        Ok(())
    })
    .await
}

pub fn build_router(state: AppState) -> Router {
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
                Json(serde_json::json!({ "status": "ok", "service": "companyos-analytics" }))
            }),
        )
        .merge(handlers::router())
        .merge(openapi::router())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}
