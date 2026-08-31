//! CompanyOS Configurable Workflow Engine (library) — Phase 3.1.
//!
//! Org-scoped workflow definitions executed via Temporal catalogue type
//! `UserWorkflow`. Activities call existing service APIs with `on_behalf_of`
//! the workflow creator — never a superuser. Simulation has zero side effects.

pub mod activities;
pub mod auth;
pub mod catalogue;
pub mod definition;
pub mod engine;
pub mod handlers;
pub mod idempotency;
pub mod openapi;
pub mod permissions;
pub mod principal;
pub mod simulate;
pub mod state;
pub mod types;

use axum::routing::get;
use axum::{Json, Router};
use state::AppState;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

/// Split a `.sql` file into statements (mirrors inventory/hr).
pub fn split_sql(sql: &str) -> Vec<String> {
    let cleaned: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut stmts = Vec::new();
    let mut buf = String::new();
    let mut in_dollar = false;
    let chars: Vec<char> = cleaned.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '$' {
            buf.push_str("$$");
            in_dollar = !in_dollar;
            i += 2;
            continue;
        }
        if chars[i] == ';' && !in_dollar {
            let stmt = buf.trim().to_string();
            if !stmt.is_empty() {
                stmts.push(stmt);
            }
            buf.clear();
            i += 1;
            continue;
        }
        buf.push(chars[i]);
        i += 1;
    }
    let stmt = buf.trim().to_string();
    if !stmt.is_empty() {
        stmts.push(stmt);
    }
    stmts
}

pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    companyos_tenancy::with_schema_migration_lock(pool, || async {
        for file in [include_str!("../migrations/001_workflow.sql")] {
            for stmt in split_sql(file) {
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
                Json(serde_json::json!({ "status": "ok", "service": "companyos-workflow" }))
            }),
        )
        .merge(handlers::router())
        .merge(openapi::router())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}
