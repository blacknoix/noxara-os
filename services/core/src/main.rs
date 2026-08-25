//! CompanyOS **core** service — Phase 0 hello vertical slice.
//!
//! Auth here is **LOCAL-ONLY** (dev headers / unsigned JWT). Never enable in production.

mod auth;
mod hello;
mod openapi;
mod state;

use std::net::SocketAddr;

use axum::routing::get;
use axum::{Json, Router};
use companyos_telemetry::init_tracing;
use state::AppState;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-core");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    let migration_sql = include_str!("../migrations/001_init.sql");
    for stmt in split_sql(migration_sql) {
        sqlx::query(stmt).execute(&pool).await?;
    }

    let state = AppState { pool };

    let x_request_id = http::HeaderName::from_static("x-request-id");

    let app = Router::new()
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
        .merge(hello::router())
        .merge(openapi::router())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state);

    let addr: SocketAddr = std::env::var("CORE_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8081".into())
        .parse()?;
    info!(%addr, "companyos-core listening (LOCAL-ONLY auth)");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn split_sql(sql: &str) -> Vec<&str> {
    sql.split(';')
        .map(str::trim)
        .filter(|s| {
            !s.is_empty()
                && !s
                    .chars()
                    .all(|c| c == '-' || c.is_whitespace() || c == '\n')
        })
        .filter(|s| {
            !s.lines()
                .all(|l| l.trim().is_empty() || l.trim_start().starts_with("--"))
        })
        .collect()
}
