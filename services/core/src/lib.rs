//! CompanyOS core library (auth + workspace + dashboard + hello) — used by the binary and integration tests.

pub mod auth;
pub mod dashboard;
pub mod governance;
pub mod hello;
pub mod openapi;
pub mod state;
pub mod workspace;

use axum::routing::get;
use axum::{Json, Router};
use state::AppState;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

/// Split a migration file into individual statements on `;`, while treating
/// `'...'` string literals, `"..."` quoted identifiers, and `$tag$...$tag$`
/// dollar-quoted bodies (e.g. `CREATE FUNCTION ... AS $$ ... $$`) as atomic —
/// semicolons inside them never terminate a statement.
pub fn split_sql(sql: &str) -> Vec<String> {
    // Strip full-line `--` comments first so semicolons inside comments don't
    // create bogus statement fragments (e.g. "tenant-owned memberships").
    let cleaned: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");

    let chars: Vec<char> = cleaned.chars().collect();
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut dollar_tag: Option<String> = None;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if let Some(tag) = dollar_tag.clone() {
            if c == '$' && chars[i + 1..].starts_with(&tag_close_chars(&tag)[..]) {
                let close = format!("${tag}$");
                current.push_str(&close);
                i += close.chars().count();
                dollar_tag = None;
                continue;
            }
            current.push(c);
            i += 1;
            continue;
        }

        if in_single_quote {
            current.push(c);
            if c == '\'' {
                if chars.get(i + 1) == Some(&'\'') {
                    current.push('\'');
                    i += 2;
                    continue;
                }
                in_single_quote = false;
            }
            i += 1;
            continue;
        }

        if in_double_quote {
            current.push(c);
            if c == '"' {
                in_double_quote = false;
            }
            i += 1;
            continue;
        }

        match c {
            '\'' => {
                in_single_quote = true;
                current.push(c);
                i += 1;
            }
            '"' => {
                in_double_quote = true;
                current.push(c);
                i += 1;
            }
            '$' => {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '$' {
                    let tag: String = chars[i + 1..j].iter().collect();
                    current.push('$');
                    current.push_str(&tag);
                    current.push('$');
                    dollar_tag = Some(tag);
                    i = j + 1;
                } else {
                    current.push(c);
                    i += 1;
                }
            }
            ';' => {
                let stmt = current.trim();
                if !stmt.is_empty() {
                    statements.push(stmt.to_string());
                }
                current.clear();
                i += 1;
            }
            _ => {
                current.push(c);
                i += 1;
            }
        }
    }

    let tail = current.trim();
    if !tail.is_empty() {
        statements.push(tail.to_string());
    }
    statements
}

fn tag_close_chars(tag: &str) -> Vec<char> {
    let mut v: Vec<char> = tag.chars().collect();
    v.push('$');
    v
}

pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    companyos_tenancy::with_schema_migration_lock(pool, || async {
        for migration in [
            include_str!("../migrations/001_init.sql"),
            include_str!("../migrations/002_auth.sql"),
            include_str!("../migrations/003_workspace.sql"),
            include_str!("../migrations/004_governance.sql"),
            include_str!("../migrations/005_sso_login.sql"),
        ] {
            for stmt in split_sql(migration) {
                companyos_tenancy::execute_migration_stmt(pool, &stmt).await?;
            }
        }
        workspace::sync_permission_catalogue(pool).await?;
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
                Json(serde_json::json!({ "status": "ok", "service": "companyos-core" }))
            }),
        )
        .merge(auth::router())
        .merge(workspace::router())
        .merge(governance::router())
        .merge(dashboard::router())
        .merge(hello::router())
        .merge(openapi::router())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}

#[cfg(test)]
mod split_sql_tests {
    use super::split_sql;

    #[test]
    fn splits_simple_statements() {
        let sql = "CREATE TABLE a (id INT); CREATE TABLE b (id INT);";
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].starts_with("CREATE TABLE a"));
        assert!(stmts[1].starts_with("CREATE TABLE b"));
    }

    #[test]
    fn keeps_dollar_quoted_function_body_intact() {
        let sql = "CREATE OR REPLACE FUNCTION f()\nRETURNS trigger\nLANGUAGE plpgsql\nAS $$\nDECLARE\n  v_x text := '';\nBEGIN\n  PERFORM pg_advisory_xact_lock(1);\n  RETURN NEW;\nEND;\n$$;\n\nCREATE TABLE t (id INT);";
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("v_x text := '';"));
        assert!(stmts[0].contains("PERFORM pg_advisory_xact_lock(1);"));
        assert!(stmts[0].ends_with("$$"));
        assert_eq!(stmts[1], "CREATE TABLE t (id INT)");
    }

    #[test]
    fn keeps_tagged_dollar_quote_intact() {
        let sql = "SELECT $tag$a;b;$tag$; SELECT 1;";
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "SELECT $tag$a;b;$tag$");
        assert_eq!(stmts[1], "SELECT 1");
    }

    #[test]
    fn semicolons_inside_string_literals_are_ignored() {
        let sql = "INSERT INTO t (s) VALUES ('a;b'); SELECT 1;";
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "INSERT INTO t (s) VALUES ('a;b')");
    }

    #[test]
    fn escaped_single_quotes_do_not_close_string_early() {
        let sql = "INSERT INTO t (s) VALUES ('it''s; fine'); SELECT 1;";
        let stmts = split_sql(sql);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "INSERT INTO t (s) VALUES ('it''s; fine')");
    }

    #[test]
    fn strips_full_line_comments() {
        let sql = "-- a comment; with semicolon\nSELECT 1;";
        let stmts = split_sql(sql);
        assert_eq!(stmts, vec!["SELECT 1".to_string()]);
    }

    #[test]
    fn governance_migration_splits_without_truncating_function_body() {
        let sql = include_str!("../migrations/004_governance.sql");
        let stmts = split_sql(sql);
        let fn_stmt = stmts
            .iter()
            .find(|s| s.contains("companyos_audit_hash_chain"))
            .expect("function statement present");
        assert!(fn_stmt.contains("RETURN NEW;"));
        assert!(fn_stmt.contains("END;"));
        assert!(fn_stmt.trim_end().ends_with("$$"));
    }
}
