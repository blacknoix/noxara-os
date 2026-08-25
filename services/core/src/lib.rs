//! CompanyOS core library (auth + workspace + dashboard + hello) — used by the binary and integration tests.

pub mod auth;
pub mod dashboard;
pub mod hello;
pub mod openapi;
pub mod state;
pub mod workspace;

use axum::routing::get;
use axum::{Json, Router};
use state::AppState;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

pub fn split_sql(sql: &str) -> Vec<String> {
    // Strip full-line `--` comments first so semicolons inside comments don't
    // create bogus statement fragments (e.g. "tenant-owned memberships").
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
    for migration in [
        include_str!("../migrations/001_init.sql"),
        include_str!("../migrations/002_auth.sql"),
        include_str!("../migrations/003_workspace.sql"),
    ] {
        for stmt in split_sql(migration) {
            sqlx::query(&stmt).execute(pool).await?;
        }
    }
    workspace::sync_permission_catalogue(pool).await?;
    Ok(())
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
                Json(serde_json::json!({ "status": "ok", "service": "companyos-core" }))
            }),
        )
        .merge(auth::router())
        .merge(workspace::router())
        .merge(dashboard::router())
        .merge(hello::router())
        .merge(openapi::router())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}
