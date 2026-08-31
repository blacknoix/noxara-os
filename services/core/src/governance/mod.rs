//! Phase 2.6 — Security & governance hardening.
//!
//! Access review (who-could-see / who-did-see), audit hash-chain
//! verification, per-org retention configuration, and organization API keys.

pub mod access_review;
pub mod api_keys;
pub mod audit_verify;
pub mod entitlement;
pub mod handlers;
pub mod retention;
pub mod types;

pub use handlers::router;

use companyos_authz::{is_allowed, PermissionId, Principal};
use companyos_errors::{AppError, ErrorCode};

use crate::auth::extract::AuthUser;
use crate::state::AppState;

/// Enforce that `principal` has `perm`, mapping a denial to 403 Forbidden.
pub(crate) fn require_perm(
    principal: &Principal,
    perm: &PermissionId,
    request_id: &str,
) -> Result<(), AppError> {
    if !is_allowed(principal, perm) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!("missing permission {}", perm.as_str()),
        ));
    }
    Ok(())
}

/// Load the caller's [`Principal`] from membership + role grants and enforce `perm`.
pub(crate) async fn authorize(
    state: &AppState,
    user: &AuthUser,
    perm: &PermissionId,
) -> Result<Principal, AppError> {
    let request_id = user.ctx.request_id.clone();
    let (principal, _policy_version, _membership_id) = crate::workspace::principal::load_principal(
        &state.pool,
        user.ctx.org_id,
        user.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    require_perm(&principal, perm, &request_id)?;
    Ok(principal)
}

pub(crate) fn internal(request_id: &str) -> impl Fn(sqlx::Error) -> AppError + '_ {
    move |e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}"))
}

pub(crate) fn tenancy_internal(
    request_id: &str,
) -> impl Fn(companyos_tenancy::TenancyError) -> AppError + '_ {
    move |e| AppError::new(ErrorCode::Internal, request_id, e.to_string())
}

pub(crate) fn outbox_internal(
    request_id: &str,
) -> impl Fn(companyos_outbox::OutboxError) -> AppError + '_ {
    move |e| {
        AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("outbox error: {e}"),
        )
    }
}

pub(crate) fn not_found(request_id: &str, what: &str) -> AppError {
    AppError::new(ErrorCode::NotFound, request_id, format!("{what} not found"))
}

pub(crate) fn validation(request_id: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, request_id, detail)
}
