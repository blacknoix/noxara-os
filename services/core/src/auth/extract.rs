//! Request identity extractors.
//!
//! Primary: signed Bearer access JWT with live membership/session checks.
//! Optional LOCAL-ONLY bypass when `COMPANYOS_LOCAL_AUTH=1` (default off).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use companyos_auth_token::verify_access_token;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::{Actor, OrgId, RequestContext};
use serde::Deserialize;
use uuid::Uuid;

use super::local_auth_enabled;
use crate::state::AppState;

const DEV_ORG_HEADER: &str = "x-companyos-dev-org-id";
const DEV_USER_HEADER: &str = "x-companyos-dev-user-id";

/// Authenticated caller (JWT primary path).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthUser {
    pub ctx: RequestContext,
    pub roles: Vec<String>,
    pub membership_id: Uuid,
    pub policy_version: i64,
    pub session_id: Uuid,
    pub family_id: Uuid,
    /// True when LOCAL-ONLY bypass was used.
    pub local_bypass: bool,
}

/// Hello-slice compatibility wrapper around [`AuthUser`].
#[derive(Debug, Clone)]
pub struct LocalAuth(pub RequestContext);

impl std::ops::Deref for LocalAuth {
    type Target = RequestContext;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Deserialize)]
struct UnsignedClaims {
    org_id: String,
    sub: String,
    #[serde(default)]
    is_ai: bool,
    on_behalf_of: Option<String>,
}

fn parse_org(s: &str) -> Result<OrgId, AppError> {
    let pub_id: PublicId = s
        .parse()
        .map_err(|_| AppError::new(ErrorCode::Unauthorized, "unknown", "invalid org_id"))?;
    OrgId::from_public(&pub_id)
        .map_err(|_| AppError::new(ErrorCode::Unauthorized, "unknown", "org_id must be org_…"))
}

fn parse_user(s: &str) -> Result<Uuid, AppError> {
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

fn request_id_from(parts: &Parts) -> String {
    parts
        .headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

async fn from_jwt(state: &AppState, token: &str, request_id: &str) -> Result<AuthUser, AppError> {
    let claims = verify_access_token(&state.auth_keys.ring, token).map_err(|e| {
        AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            format!("invalid access token: {e}"),
        )
    })?;

    #[allow(clippy::type_complexity)]
    let row: Option<(
        Option<chrono::DateTime<chrono::Utc>>,
        i64,
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
            SELECT m.revoked_at, m.policy_version, m.role, s.revoked_at
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

    let Some((mem_revoked, policy_version, role, sess_revoked)) = row else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "membership or session not found",
        ));
    };

    if mem_revoked.is_some() {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "membership revoked",
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
    let ctx = RequestContext::new(org_id, Actor::human(claims.user_id), request_id.to_string());
    Ok(AuthUser {
        ctx,
        roles: vec![role],
        membership_id: claims.membership_id,
        policy_version,
        session_id: claims.sid,
        family_id: claims.family_id,
        local_bypass: false,
    })
}

fn from_local_headers(parts: &Parts, request_id: &str) -> Result<Option<AuthUser>, AppError> {
    let org_hdr = parts
        .headers
        .get(DEV_ORG_HEADER)
        .and_then(|v| v.to_str().ok());
    let user_hdr = parts
        .headers
        .get(DEV_USER_HEADER)
        .and_then(|v| v.to_str().ok());

    if let (Some(org_s), Some(user_s)) = (org_hdr, user_hdr) {
        tracing::warn!(
            request_id = %request_id,
            "LOCAL-ONLY auth via X-CompanyOS-Dev-* headers — not for production"
        );
        let org_id = parse_org(org_s)
            .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail))?;
        let user_id = parse_user(user_s)
            .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail))?;
        let ctx = RequestContext::new(org_id, Actor::human(user_id), request_id.to_string());
        return Ok(Some(AuthUser {
            ctx,
            roles: vec!["owner".into()],
            membership_id: Uuid::nil(),
            policy_version: 0,
            session_id: Uuid::nil(),
            family_id: Uuid::nil(),
            local_bypass: true,
        }));
    }
    Ok(None)
}

fn from_unsigned_bearer(token: &str, request_id: &str) -> Result<Option<AuthUser>, AppError> {
    // Reject three-part JWTs here — those must be verified cryptographically.
    if token.matches('.').count() == 2 {
        return Ok(None);
    }
    let claims = match decode_unsigned(token) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    tracing::warn!(
        request_id = %request_id,
        "LOCAL-ONLY auth via unsigned bearer — not for production"
    );
    let org_id = parse_org(&claims.org_id)
        .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail))?;
    let user_id = parse_user(&claims.sub)
        .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail))?;
    let actor = if claims.is_ai {
        let human = claims
            .on_behalf_of
            .as_deref()
            .map(parse_user)
            .transpose()
            .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail))?
            .unwrap_or(user_id);
        Actor::ai_on_behalf_of(user_id, human)
    } else {
        Actor::human(user_id)
    };
    let ctx = RequestContext::new(org_id, actor, request_id.to_string());
    Ok(Some(AuthUser {
        ctx,
        roles: vec!["member".into()],
        membership_id: Uuid::nil(),
        policy_version: 0,
        session_id: Uuid::nil(),
        family_id: Uuid::nil(),
        local_bypass: true,
    }))
}

fn decode_unsigned(token: &str) -> Result<UnsignedClaims, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(token))
        .map_err(|e| format!("invalid unsigned token: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("invalid unsigned token json: {e}"))
}

async fn extract_auth_user(parts: &mut Parts, state: &AppState) -> Result<AuthUser, AppError> {
    let request_id = request_id_from(parts);

    let bearer = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(|s| s.to_string()));

    if let Some(token) = bearer.as_deref() {
        match from_jwt(state, token, &request_id).await {
            Ok(user) => return Ok(user),
            Err(jwt_err) => {
                if local_auth_enabled() {
                    if let Some(local) = from_unsigned_bearer(token, &request_id)? {
                        return Ok(local);
                    }
                }
                // If local headers are set, prefer them only when JWT failed and local is on.
                if local_auth_enabled() {
                    if let Some(local) = from_local_headers(parts, &request_id)? {
                        return Ok(local);
                    }
                }
                return Err(jwt_err);
            }
        }
    }

    if local_auth_enabled() {
        if let Some(local) = from_local_headers(parts, &request_id)? {
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

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        extract_auth_user(parts, state).await
    }
}

impl FromRequestParts<AppState> for LocalAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = extract_auth_user(parts, state).await?;
        Ok(LocalAuth(user.ctx))
    }
}

/// Encode a LOCAL-ONLY unsigned token for tests/scripts.
#[allow(dead_code)]
pub fn encode_local_token(org_id: &str, user_id: &str) -> String {
    let json = serde_json::json!({ "org_id": org_id, "sub": user_id });
    URL_SAFE_NO_PAD.encode(json.to_string().as_bytes())
}
