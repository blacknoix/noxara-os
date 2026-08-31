//! Phase 3.4 — marketplace skeleton.
//!
//! Design invariants enforced across this module:
//!
//! * **Consent is the only authority.** Tokens are issued with exactly the
//!   install's `consented_scopes`, which must be a subset of the listing's
//!   `requested_scopes` *and* of the installing principal's own permissions.
//!   Widening consent revokes existing tokens and re-issues.
//! * **No first-party special case.** `listing_kind` and `connector_key` are
//!   data, not control flow. First-party connectors and third-party apps share
//!   [`install::create_install`], [`tokens::issue_tokens`] and
//!   [`install::revoke_install`]. The `/api/v1/integrations/...` routes are a
//!   thin alias over the same functions.
//! * **Secrets are never stored or logged in plaintext.** Client secrets and
//!   app tokens are SHA-256 hashed via `companyos_auth_token::hash_token` and
//!   the plaintext is returned exactly once.

pub mod auth;
pub mod handlers;
pub mod install;
pub mod listings;
pub mod oauth;
pub mod principal;
pub mod review;
pub mod seed;
pub mod tokens;
pub mod types;

use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::{Actor, OrgId};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// Aggregate name used for every marketplace outbox event.
pub const AGGREGATE: &str = "marketplace";

pub(crate) fn internal(request_id: &str) -> impl Fn(sqlx::Error) -> AppError + '_ {
    move |error| AppError::new(ErrorCode::Internal, request_id, error.to_string())
}

pub(crate) fn not_found(request_id: &str, resource: &str) -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        request_id,
        format!("{resource} not found"),
    )
}

pub(crate) fn validation(request_id: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, request_id, detail)
}

pub(crate) fn forbidden(request_id: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Forbidden, request_id, detail)
}

pub(crate) fn unauthorized(request_id: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Unauthorized, request_id, detail)
}

pub(crate) fn conflict(request_id: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Conflict, request_id, detail)
}

pub(crate) fn parse_public_id(kind: IdKind, raw: &str, request_id: &str) -> Result<Uuid, AppError> {
    let public: PublicId = raw
        .parse()
        .map_err(|_| validation(request_id, format!("invalid {} id", kind.prefix())))?;
    if public.kind() != kind {
        return Err(validation(
            request_id,
            format!("id must use the {} prefix", kind.prefix()),
        ));
    }
    Ok(public.uuid())
}

pub(crate) async fn set_org(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    request_id: &str,
) -> Result<(), AppError> {
    companyos_tenancy::set_session_org_id(tx, org_id)
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))
}

/// Bind the RLS bypass key used by unauthenticated token/code lookups.
pub(crate) async fn set_token_lookup(
    tx: &mut Transaction<'_, Postgres>,
    request_id: &str,
) -> Result<(), AppError> {
    companyos_tenancy::set_marketplace_token_lookup(tx)
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))
}

/// Bind the catalogue-seed RLS bypass key (bootstrap / tests only).
pub(crate) async fn set_seed_flag(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.marketplace_seed', '1', true)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Decode a JSONB text array, tolerating `null` and non-string members.
pub(crate) fn string_array(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Insert a marketplace outbox event in the caller's transaction.
pub(crate) async fn emit_event(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    actor: Actor,
    event_type: &str,
    payload: serde_json::Value,
    request_id: &str,
) -> Result<(), AppError> {
    let envelope = EventEnvelope::new(
        org_id,
        Context::Admin,
        AGGREGATE,
        event_type,
        1,
        actor,
        payload,
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    Ok(())
}

pub(crate) fn org_public(org_id: Uuid) -> String {
    OrgId::new(org_id).to_public().as_str()
}
