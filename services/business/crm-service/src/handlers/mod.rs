//! HTTP handlers for `/api/v1/sales/...`.
//!
//! Split by aggregate for readability; [`router`] merges every sub-router.

pub mod activities;
pub mod contacts;
pub mod customers;
pub mod deals;
pub mod imports;
pub mod leads;
pub mod pipelines;
pub mod products;
pub mod quotes;
pub mod reports;

use axum::http::HeaderMap;
use axum::Router;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use uuid::Uuid;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(pipelines::router())
        .merge(customers::router())
        .merge(contacts::router())
        .merge(leads::router())
        .merge(deals::router())
        .merge(activities::router())
        .merge(products::router())
        .merge(quotes::router())
        .merge(imports::router())
        .merge(reports::router())
}

/// Map a `sqlx::Error` to an internal `AppError`, capturing `request_id`.
pub(crate) fn internal(request_id: &str) -> impl Fn(sqlx::Error) -> AppError + '_ {
    move |e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}"))
}

pub(crate) fn not_found(request_id: &str, what: &str) -> AppError {
    AppError::new(ErrorCode::NotFound, request_id, format!("{what} not found"))
}

pub(crate) fn validation(request_id: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, request_id, detail)
}

pub(crate) fn conflict(request_id: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Conflict, request_id, detail)
}

/// Parse a path-param public id and assert it matches `kind`.
pub(crate) fn parse_public_id(kind: IdKind, raw: &str, request_id: &str) -> Result<Uuid, AppError> {
    let pid: PublicId = raw
        .parse()
        .map_err(|_| validation(request_id, format!("invalid id: {raw}")))?;
    if pid.kind() != kind {
        return Err(validation(
            request_id,
            format!("id {raw} is not a {kind:?} id"),
        ));
    }
    Ok(pid.uuid())
}

/// Parse an optional public id field (request body) of a given kind.
pub(crate) fn parse_optional_public_id(
    kind: IdKind,
    raw: Option<&str>,
    request_id: &str,
) -> Result<Option<Uuid>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => parse_public_id(kind, s, request_id).map(Some),
    }
}

/// Clamp `(limit, offset)` query params to sane defaults.
pub(crate) fn normalize_paging(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);
    (limit, offset)
}

/// Parse the `If-Match` header as an expected `version` integer (quotes optional).
pub(crate) fn if_match_version(headers: &HeaderMap) -> Option<i32> {
    headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().trim_matches('"').parse::<i32>().ok())
}

/// True if `err` is a Postgres unique-violation (SQLSTATE 23505) on the named
/// constraint/index — used to turn a hard DB constraint into a clean RFC 9457
/// 409 instead of leaking a 500 for an otherwise-expected race/duplicate.
pub(crate) fn is_unique_violation(err: &sqlx::Error, constraint: &str) -> bool {
    match err {
        sqlx::Error::Database(db_err) => {
            db_err.code().as_deref() == Some("23505") && db_err.constraint() == Some(constraint)
        }
        _ => false,
    }
}
