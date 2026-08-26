//! HTTP handlers for `/api/v1/operations/...`.

pub mod board;
pub mod calendar;
pub mod comments;
pub mod events;
pub mod my_work;
pub mod projects;
pub mod summary;
pub mod tasks;

use axum::http::HeaderMap;
use axum::Router;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use uuid::Uuid;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(projects::router())
        .merge(tasks::router())
        .merge(comments::router())
        .merge(board::router())
        .merge(my_work::router())
        .merge(calendar::router())
        .merge(summary::router())
        .merge(events::router())
}

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

pub(crate) fn parse_user_ref(raw: &str, request_id: &str) -> Result<Uuid, AppError> {
    if let Ok(u) = Uuid::parse_str(raw) {
        return Ok(u);
    }
    parse_public_id(IdKind::User, raw, request_id)
}

pub(crate) fn user_public(uuid: Uuid) -> String {
    PublicId::new(IdKind::User, uuid).as_str()
}

pub(crate) fn normalize_paging(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);
    (limit, offset)
}

pub(crate) fn if_match_version(headers: &HeaderMap) -> Option<i32> {
    headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().trim_matches('"').parse::<i32>().ok())
}

pub(crate) fn require_if_match(headers: &HeaderMap, request_id: &str) -> Result<i32, AppError> {
    if_match_version(headers).ok_or_else(|| {
        validation(
            request_id,
            "If-Match header with integer version is required",
        )
    })
}
