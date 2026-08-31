//! HTTP handlers for `/api/v1/workflows/...`.

#![allow(clippy::type_complexity)]

pub mod catalogue;
pub mod definitions;
pub mod instances;
pub mod monitor;
pub mod simulate;

use axum::http::HeaderMap;
use axum::Router;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use uuid::Uuid;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(catalogue::router())
        .merge(definitions::router())
        .merge(instances::router())
        .merge(simulate::router())
        .merge(monitor::router())
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

pub(crate) fn parse_public_id(kind: IdKind, raw: &str, request_id: &str) -> Result<Uuid, AppError> {
    let pid: PublicId = raw
        .parse()
        .map_err(|_| validation(request_id, format!("invalid id: {raw}")))?;
    if pid.kind() != kind {
        return Err(validation(
            request_id,
            format!("id {raw} is not a {} id", kind.prefix()),
        ));
    }
    Ok(pid.uuid())
}

pub(crate) fn user_public(uuid: Uuid) -> String {
    PublicId::new(IdKind::User, uuid).as_str()
}

pub(crate) fn require_idempotency(
    headers: &HeaderMap,
    request_id: &str,
) -> Result<String, AppError> {
    crate::idempotency::header_key(headers).ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "Idempotency-Key header is required",
        )
    })
}

pub(crate) async fn set_org(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: companyos_tenancy::OrgId,
    request_id: &str,
) -> Result<(), AppError> {
    companyos_tenancy::set_session_org_id(tx, org_id)
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                request_id,
                format!("set org session: {e}"),
            )
        })
}
