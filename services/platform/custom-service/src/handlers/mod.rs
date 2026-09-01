//! HTTP handlers for `/api/v1/custom/...`.

pub mod entities;
pub mod layouts;
pub mod packages;
pub mod records;
pub mod scripts;
pub mod views;

use axum::Router;
use companyos_authz::{PermissionId, Principal};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::OrgId;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(entities::router())
        .merge(records::router())
        .merge(views::router())
        .merge(layouts::router())
        .merge(scripts::router())
        .merge(packages::router())
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
            format!("id {raw} is not a {} id", kind.prefix()),
        ));
    }
    Ok(pid.uuid())
}

pub(crate) async fn set_org(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: OrgId,
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

/// Load principal and enforce permission unless local-auth bypass is active.
pub(crate) async fn require_perm(
    state: &AppState,
    auth: &AuthCtx,
    permission: PermissionId,
) -> Result<(), AppError> {
    if auth.local_bypass {
        return Ok(());
    }
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        &auth.ctx.request_id,
    )
    .await?;
    enforce(&principal, permission, &auth.ctx.request_id)
}

pub(crate) async fn load_authz_principal(
    state: &AppState,
    auth: &AuthCtx,
) -> Result<Option<Principal>, AppError> {
    if auth.local_bypass {
        return Ok(None);
    }
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        &auth.ctx.request_id,
    )
    .await?;
    Ok(Some(principal))
}

pub(crate) fn enforce_opt(
    principal: &Option<Principal>,
    permission: PermissionId,
    request_id: &str,
) -> Result<(), AppError> {
    match principal {
        None => Ok(()),
        Some(p) => enforce(p, permission, request_id),
    }
}
