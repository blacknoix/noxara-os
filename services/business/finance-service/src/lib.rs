//! CompanyOS Finance service (library) — Phase 1.5–2.4.
//!
//! Bounded context: **Finance** (`finance_*` tables, `Context::Finance` events).
//! All routes are mounted under `/api/v1/finance/...`. This service is a
//! standalone network service (own binary, own port) even though it shares
//! the core Postgres database — it reads `organization` / `membership` /
//! `role_permission` / `org_role` for authz, and **never** reads or writes
//! CRM/`sales_*` tables. Customer data arrives via event projection or
//! quote snapshots. `companyos_authz` remains the sole policy decision point.
//! HR/payroll posts journals only through Finance HTTP APIs.

pub mod audit;
pub mod auth;
pub mod handlers;
pub mod idempotency;
pub mod invoice_math;
pub mod journal;
pub mod numbering;
pub mod openapi;
pub mod periods;
pub mod principal;
pub mod projection;
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
/// statement fragments. Respects `$$ … $$` dollar-quotes so PL/pgSQL
/// function bodies are not split on internal semicolons.
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

/// Run the Finance schema migration. Assumes `companyos_core::migrate` (or
/// equivalent) has already created `organization` / `user_identity` /
/// `membership` / `role_permission` / `org_role` / `outbox_event` /
/// `audit_entry` on this same database — this migration only adds
/// `finance_*` tables.
pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    companyos_tenancy::with_schema_migration_lock(pool, || async {
        for migration in [
            include_str!("../migrations/001_finance.sql"),
            include_str!("../migrations/002_approval_link.sql"),
            include_str!("../migrations/003_payroll_journal.sql"),
            include_str!("../migrations/004_accounting.sql"),
            include_str!("../migrations/005_vendor_bills.sql"),
            include_str!("../migrations/006_finance_depth.sql"),
        ] {
            for stmt in split_sql(migration) {
                companyos_tenancy::execute_migration_stmt(pool, &stmt).await?;
            }
        }
        Ok(())
    })
    .await
}

/// Build the full Finance axum router: `/api/v1/finance/...` handlers +
/// OpenAPI + health checks.
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
                Json(serde_json::json!({ "status": "ok", "service": "companyos-finance" }))
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

    #[test]
    fn split_sql_preserves_dollar_quoted_function_bodies() {
        let sql = r#"
CREATE OR REPLACE FUNCTION foo() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'nope';
END;
$$ LANGUAGE plpgsql;
CREATE TABLE bar (id UUID);
"#;
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("RAISE EXCEPTION"));
        assert!(stmts[0].contains("$$"));
        assert!(stmts[1].starts_with("CREATE TABLE"));
    }
}
