//! CompanyOS Inventory & Procurement service (library) — Phase 2.5.
//!
//! Bounded context: **Inventory** (`inventory_*` tables, `Context::Inventory`
//! events). Routes mount under `/api/v1/inventory/...`. Standalone network
//! service (own binary, own port) sharing core Postgres — reads
//! `organization` / `membership` / `role_permission` for authz via
//! `companyos_authz` (the sole PDP), and **never** touches `people_*` (HR) or
//! `finance_*` tables directly. All finance postings (journals, vendor bills)
//! go through [`finance_client`] over HTTP; approval routing for purchase
//! requests goes through [`approvals_client`] to `companyos-project`.
//!
//! Valuation is Weighted Average — see
//! `docs/adrs/023-inventory-valuation-wavg.md` and [`valuation`] / [`stock`].

pub mod approvals_client;
pub mod audit;
pub mod auth;
pub mod finance_client;
pub mod handlers;
pub mod idempotency;
pub mod openapi;
pub mod principal;
pub mod scope;
pub mod state;
pub mod stock;
pub mod types;
pub mod valuation;

use axum::routing::get;
use axum::{Json, Router};
use state::AppState;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

/// Split a `.sql` file into individual statements, stripping full-line `--`
/// comments first so semicolons inside comments don't create bogus
/// statement fragments. Respects `$$ … $$` dollar-quotes. Mirrors
/// `companyos_hr::split_sql`.
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

/// Run the Inventory schema migration. Assumes core migrate has already
/// created shared tables (`organization`, `membership`, `outbox_event`, …).
pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    companyos_tenancy::with_schema_migration_lock(pool, || async {
        for file in [include_str!("../migrations/001_inventory.sql")] {
            for stmt in split_sql(file) {
                companyos_tenancy::execute_migration_stmt(pool, &stmt).await?;
            }
        }
        Ok(())
    })
    .await
}

/// Build the full Inventory axum router.
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
                Json(serde_json::json!({ "status": "ok", "service": "companyos-inventory" }))
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
