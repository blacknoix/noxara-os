//! **LOCAL-ONLY** authentication for Phase 0.
//!
//! Supported mechanisms (never for production):
//! 1. Headers: `X-CompanyOS-Dev-Org-Id`, `X-CompanyOS-Dev-User-Id`
//! 2. `Authorization: Bearer <base64url(json)>` unsigned JWT-shaped payload with `org_id` + `sub`
//!
//! Marked clearly in logs and responses when used.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::{Actor, OrgId, RequestContext};
use serde::Deserialize;
use uuid::Uuid;

const DEV_ORG_HEADER: &str = "x-companyos-dev-org-id";
const DEV_USER_HEADER: &str = "x-companyos-dev-user-id";

/// Extracted local-only request identity.
#[derive(Debug, Clone)]
pub struct LocalAuth(pub RequestContext);

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

impl FromRequestParts<crate::state::AppState> for LocalAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let request_id = request_id_from(parts);

        // Prefer explicit LOCAL-ONLY dev headers.
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
                .map_err(|e| AppError::new(e.code, request_id.clone(), e.detail))?;
            let user_id = parse_user(user_s)
                .map_err(|e| AppError::new(e.code, request_id.clone(), e.detail))?;
            let ctx = RequestContext::new(org_id, Actor::human(user_id), request_id);
            return Ok(LocalAuth(ctx));
        }

        // Unsigned bearer payload (LOCAL-ONLY).
        if let Some(auth) = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            if let Some(token) = auth.strip_prefix("Bearer ") {
                tracing::warn!(
                    request_id = %request_id,
                    "LOCAL-ONLY auth via unsigned bearer — not for production"
                );
                let claims = decode_unsigned(token)
                    .map_err(|d| AppError::new(ErrorCode::Unauthorized, request_id.clone(), d))?;
                let org_id = parse_org(&claims.org_id)
                    .map_err(|e| AppError::new(e.code, request_id.clone(), e.detail))?;
                let user_id = parse_user(&claims.sub)
                    .map_err(|e| AppError::new(e.code, request_id.clone(), e.detail))?;
                let actor = if claims.is_ai {
                    let human = claims
                        .on_behalf_of
                        .as_deref()
                        .map(parse_user)
                        .transpose()
                        .map_err(|e| AppError::new(e.code, request_id.clone(), e.detail))?
                        .unwrap_or(user_id);
                    Actor::ai_on_behalf_of(user_id, human)
                } else {
                    Actor::human(user_id)
                };
                let ctx = RequestContext::new(org_id, actor, request_id);
                return Ok(LocalAuth(ctx));
            }
        }

        Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "LOCAL-ONLY auth required: X-CompanyOS-Dev-Org-Id + X-CompanyOS-Dev-User-Id, or unsigned Bearer",
        ))
    }
}

fn decode_unsigned(token: &str) -> Result<UnsignedClaims, String> {
    // Accept raw base64url JSON or fake JWT header.payload.sig (sig ignored).
    let payload = if token.matches('.').count() == 2 {
        token.split('.').nth(1).unwrap_or(token)
    } else {
        token
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(payload))
        .map_err(|e| format!("invalid unsigned token: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("invalid unsigned token json: {e}"))
}

/// Encode a LOCAL-ONLY unsigned token for tests/scripts.
#[allow(dead_code)]
pub fn encode_local_token(org_id: &str, user_id: &str) -> String {
    let json = serde_json::json!({ "org_id": org_id, "sub": user_id });
    URL_SAFE_NO_PAD.encode(json.to_string().as_bytes())
}
