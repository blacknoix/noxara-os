//! CompanyOS Projects & Tasks service (library) — Phase 1.6+1.7.
//!
//! Bounded context: **Operations** (`operations_*` tables, `Context::Operations`
//! events). Routes mount under `/api/v1/operations/...`. Standalone network
//! service (own binary, own port) sharing core Postgres — reads
//! `organization` / `membership` / `role_permission` / `org_role` for authz,
//! and **never** reads or writes CRM/`sales_*` or Finance tables. Customer /
//! deal links are opaque UUIDs (+ optional public ids) from the request body
//! or DealWon event projection. Approval records live here; finance/CRM call
//! the approval API rather than reading operations tables.
//! `companyos_authz` remains the sole PDP.

pub mod approvals;
pub mod audit;
pub mod auth;
pub mod handlers;
pub mod idempotency;
pub mod mentions;
pub mod openapi;
pub mod principal;
pub mod scope;
pub mod state;
pub mod types;

use axum::routing::get;
use axum::{Json, Router};
use state::AppState;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

/// Split a `.sql` file into individual statements, stripping full-line `--`
/// comments first so semicolons inside comments don't create bogus
/// statement fragments. Respects `$$ … $$` dollar-quotes.
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

/// Run the Operations schema migration. Assumes core migrate has already
/// created shared tables (`organization`, `membership`, `outbox_event`, …).
pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    for migration in [
        include_str!("../migrations/001_operations.sql"),
        include_str!("../migrations/002_approvals.sql"),
    ] {
        for stmt in split_sql(migration) {
            sqlx::query(&stmt).execute(pool).await?;
        }
    }
    Ok(())
}

/// Build the full Operations axum router.
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
                Json(serde_json::json!({ "status": "ok", "service": "companyos-project" }))
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
    fn split_sql_ignores_comment_lines_with_semicolons() {
        let sql =
            "-- a comment; with a semicolon\nCREATE TABLE t (id UUID);\n-- another;\nSELECT 1;";
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].starts_with("CREATE TABLE"));
        assert!(stmts[1].starts_with("SELECT"));
    }
}
