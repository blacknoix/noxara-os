//! Request identity extraction for companyos-workflow.
//!
//! Primary path: signed Bearer access JWT (same `AUTH_JWT_SECRET` / keyring
//! convention as `companyos-core`), with a live membership + session check —
//! this mirrors `services/core/src/auth/extract.rs` but only reads the
//! `membership` / `auth_session` tables it needs (never `people_*`,
//! `finance_*`, or `sales_*` tables).
//!
//! Also accepted:
//! - Gateway-forwarded `x-companyos-org-id` / `x-companyos-user-id` headers
//!   alongside a Bearer token — cross-checked against the verified JWT
//!   claims (defense in depth), never trusted on their own.
//! - LOCAL-ONLY `x-companyos-dev-org-id` / `x-companyos-dev-user-id` headers
//!   when `COMPANYOS_LOCAL_AUTH=1` (default off; dev/test only).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use companyos_auth_token::verify_access_token;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::{Actor, OrgId, RequestContext};
use uuid::Uuid;

use crate::state::AppState;

const DEV_ORG_HEADER: &str = "x-companyos-dev-org-id";
const DEV_USER_HEADER: &str = "x-companyos-dev-user-id";
const GW_ORG_HEADER: &str = "x-companyos-org-id";
const GW_USER_HEADER: &str = "x-companyos-user-id";

/// Authenticated caller for an Workflow request.
#[derive(Debug, Clone)]
pub struct AuthCtx {
    pub ctx: RequestContext,
    pub roles: Vec<String>,
    pub membership_id: Uuid,
    pub policy_version: i64,
    /// True when a LOCAL-ONLY bypass was used (never in production).
    pub local_bypass: bool,
}

/// Build the verifier keyring from `AUTH_JWT_SECRET` (falls back to an
/// ephemeral dev secret, matching `companyos-core`'s `build_keyring`).
/// Workflow never mints tokens, so this only needs to *verify* — but a
/// shared secret with core is required for tokens minted there to validate
/// here.
pub fn build_keyring() -> companyos_auth_token::KeyRing {
    let secret = std::env::var("AUTH_JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!("AUTH_JWT_SECRET unset — using ephemeral in-process secret");
        format!("dev-only-{}", Uuid::now_v7())
    });
    companyos_auth_token::KeyRing::from_secret(secret)
}

/// Load any additional signing keys core has rotated in via
/// `jwks_signing_key` (read-only — Workflow never writes this table). Lets
/// tokens minted after a core-side key rotation still verify here.
pub async fn load_rotated_keys(
    pool: &sqlx::PgPool,
    ring: &companyos_auth_token::KeyRing,
) -> Result<(), sqlx::Error> {
    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT kid, secret_material, is_active FROM jwks_signing_key WHERE retired_at IS NULL",
    )
    .fetch_all(pool)
    .await?;
    for (kid, secret, is_active) in rows {
        ring.upsert(companyos_auth_token::SigningKey {
            kid,
            secret,
            active: is_active,
        });
    }
    Ok(())
}

pub fn local_auth_enabled() -> bool {
    matches!(
        std::env::var("COMPANYOS_LOCAL_AUTH").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

pub fn request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

pub fn parse_org_public_id(s: &str) -> Result<OrgId, AppError> {
    let pub_id: PublicId = s
        .parse()
        .map_err(|_| AppError::new(ErrorCode::Unauthorized, "unknown", "invalid org_id"))?;
    OrgId::from_public(&pub_id)
        .map_err(|_| AppError::new(ErrorCode::Unauthorized, "unknown", "org_id must be org_…"))
}

pub fn parse_user_public_id(s: &str) -> Result<Uuid, AppError> {
    if let Ok(u) = Uuid::parse_str(s) {
        return Ok(u);
    }
    let pub_id: PublicId = s
        .parse()
        .map_err(|_| AppError::new(ErrorCode::Unauthorized, "unknown", "invalid user id"))?;
    if pub_id.kind() != IdKind::User {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            "unknown",
            "user id must be usr_… or uuid",
        ));
    }
    Ok(pub_id.uuid())
}

async fn from_jwt(
    state: &AppState,
    token: &str,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<AuthCtx, AppError> {
    let claims = verify_access_token(&state.keyring, token).map_err(|e| {
        AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            format!("invalid access token: {e}"),
        )
    })?;

    // Defense in depth: when the gateway also forwarded resolved identity
    // headers, they must agree with the verified JWT claims.
    if let Some(hv) = headers.get(GW_ORG_HEADER).and_then(|v| v.to_str().ok()) {
        if hv != claims.org_id {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                request_id,
                "gateway org header does not match access token",
            ));
        }
    }
    if let Some(hv) = headers.get(GW_USER_HEADER).and_then(|v| v.to_str().ok()) {
        if parse_user_public_id(hv).ok() != Some(claims.user_id) {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                request_id,
                "gateway user header does not match access token",
            ));
        }
    }

    #[allow(clippy::type_complexity)]
    let row: Option<(
        Option<chrono::DateTime<chrono::Utc>>,
        i64,
        String,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = {
        let mut tx = state
            .pool
            .begin()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        companyos_tenancy::set_session_org_id(&mut tx, OrgId::new(claims.org_uuid))
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        let row = sqlx::query_as(
            r#"
            SELECT m.revoked_at, m.policy_version, m.role, m.status, s.revoked_at
            FROM membership m
            JOIN auth_session s ON s.id = $3
            WHERE m.id = $1 AND m.user_id = $2 AND m.org_id = $4
            "#,
        )
        .bind(claims.membership_id)
        .bind(claims.user_id)
        .bind(claims.sid)
        .bind(claims.org_uuid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        row
    };

    let Some((mem_revoked, policy_version, role, status, sess_revoked)) = row else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "membership or session not found",
        ));
    };

    if mem_revoked.is_some() || status == "revoked" {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "membership revoked",
        ));
    }
    if status == "suspended" {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "membership suspended",
        ));
    }
    if sess_revoked.is_some() {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "session revoked",
        ));
    }
    if policy_version != claims.policy_version {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "policy_version stale — re-authenticate or switch-org",
        ));
    }

    let org_id = OrgId::new(claims.org_uuid);
    let actor = actor_from_headers(headers, claims.user_id, request_id)?;
    let ctx = RequestContext::new(org_id, actor, request_id.to_string());
    Ok(AuthCtx {
        ctx,
        roles: vec![role],
        membership_id: claims.membership_id,
        policy_version,
        local_bypass: false,
    })
}

/// AI confirm path: same user JWT + explicit AI-on-behalf-of headers (no privilege escalation).
fn actor_from_headers(
    headers: &HeaderMap,
    user_id: Uuid,
    request_id: &str,
) -> Result<Actor, AppError> {
    let is_ai = headers
        .get("x-companyos-actor-is-ai")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s == "true" || s == "1");
    if !is_ai {
        return Ok(Actor::human(user_id));
    }
    let on_behalf = match headers
        .get("x-companyos-on-behalf-of")
        .and_then(|v| v.to_str().ok())
    {
        Some(s) => parse_user_public_id(s)
            .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail))?,
        None => user_id,
    };
    if on_behalf != user_id {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "AI on_behalf_of must match the authenticated user",
        ));
    }
    Ok(Actor::ai_on_behalf_of(user_id, on_behalf))
}

fn from_local_headers(headers: &HeaderMap, request_id: &str) -> Result<Option<AuthCtx>, AppError> {
    let org_hdr = headers.get(DEV_ORG_HEADER).and_then(|v| v.to_str().ok());
    let user_hdr = headers.get(DEV_USER_HEADER).and_then(|v| v.to_str().ok());

    if let (Some(org_s), Some(user_s)) = (org_hdr, user_hdr) {
        tracing::warn!(
            request_id = %request_id,
            "LOCAL-ONLY auth via X-CompanyOS-Dev-* headers — not for production"
        );
        let org_id = parse_org_public_id(org_s)
            .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail))?;
        let user_id = parse_user_public_id(user_s)
            .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail))?;
        let ctx = RequestContext::new(org_id, Actor::human(user_id), request_id.to_string());
        return Ok(Some(AuthCtx {
            ctx,
            roles: vec!["owner".into()],
            membership_id: Uuid::nil(),
            policy_version: 0,
            local_bypass: true,
        }));
    }
    Ok(None)
}

async fn extract_auth_ctx(parts: &mut Parts, state: &AppState) -> Result<AuthCtx, AppError> {
    let request_id = request_id(&parts.headers);

    let bearer = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(|s| s.to_string()));

    if let Some(token) = bearer.as_deref() {
        return from_jwt(state, token, &parts.headers, &request_id).await;
    }

    if local_auth_enabled() {
        if let Some(local) = from_local_headers(&parts.headers, &request_id)? {
            return Ok(local);
        }
    }

    Err(AppError::new(
        ErrorCode::Unauthorized,
        request_id,
        if local_auth_enabled() {
            "auth required: Bearer access token, or LOCAL-ONLY X-CompanyOS-Dev-* headers"
        } else {
            "Bearer access token required"
        },
    ))
}

impl FromRequestParts<AppState> for AuthCtx {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        extract_auth_ctx(parts, state).await
    }
}
