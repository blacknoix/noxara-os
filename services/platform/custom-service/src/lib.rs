//! CompanyOS Low-code builder service (library) — Phase 4.4.
//!
//! Custom entity definitions, records, formulas, scripts, views, layouts,
//! and versioned customisation packages under `/api/v1/custom/...`.

pub mod audit;
pub mod auth;
pub mod formula;
pub mod handlers;
pub mod openapi;
pub mod permissions;
pub mod principal;
pub mod sandbox;
pub mod search_doc;
pub mod state;
pub mod types;

use axum::routing::get;
use axum::{Json, Router};
use state::AppState;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

/// Split a `.sql` file into statements, respecting `$$` dollar-quoting (workflow pattern).
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

/// Run custom-service schema migrations (`001` then `002`).
pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    companyos_tenancy::with_schema_migration_lock(pool, || async {
        for file in [
            include_str!("../migrations/001_custom.sql"),
            include_str!("../migrations/002_platform_bump.sql"),
        ] {
            for stmt in split_sql(file) {
                companyos_tenancy::execute_migration_stmt(pool, &stmt).await?;
            }
        }
        Ok(())
    })
    .await
}

/// Build the full custom axum router: `/api/v1/custom/...` + OpenAPI + health.
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
                Json(serde_json::json!({ "status": "ok", "service": "companyos-custom" }))
            }),
        )
        .merge(handlers::router())
        .merge(openapi::router())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_sql_respects_dollar_quotes() {
        let sql = "CREATE FUNCTION f() RETURNS void AS $$\nBEGIN\nSELECT 1;\nEND;\n$$ LANGUAGE plpgsql;";
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("SELECT 1;"));
    }

    #[test]
    fn split_sql_ignores_comment_semicolons() {
        let sql = "-- note; ignore\nCREATE TABLE t (id UUID);\nSELECT 1;";
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 2);
    }
}
