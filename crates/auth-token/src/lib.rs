//! Org-scoped access tokens for CompanyOS.
//!
//! Access JWTs always carry `org_id`. A token minted for org A must never be
//! accepted as authority for org B. Org switching issues a **new** access token
//! via `POST /auth/switch-org` — never a client-side header swap.
//!
//! Refresh tokens are opaque (not JWTs) and live in an httpOnly Secure cookie.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

/// Cookie name for the refresh token (httpOnly, Secure, SameSite=Lax).
pub const REFRESH_COOKIE_NAME: &str = "companyos_refresh";

/// Default access token TTL.
pub const ACCESS_TOKEN_TTL_SECS: i64 = 900; // 15 minutes

/// Default refresh token TTL.
pub const REFRESH_TOKEN_TTL_SECS: i64 = 60 * 60 * 24 * 30; // 30 days

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("jwt error: {0}")]
    Jwt(String),
    #[error("unknown signing key kid={0}")]
    UnknownKid(String),
    #[error("no active signing key")]
    NoActiveKey,
    #[error("token missing required claim")]
    MissingClaim,
}

/// Claims embedded in every access token. `org_id` is mandatory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessClaims {
    /// User public id (`usr_…`) or raw uuid string — prefer public id.
    pub sub: String,
    /// Internal user uuid.
    pub user_id: Uuid,
    /// Organization public id (`org_…`).
    pub org_id: String,
    /// Internal org uuid.
    pub org_uuid: Uuid,
    /// Membership row id.
    pub membership_id: Uuid,
    /// Roles at mint time.
    pub roles: Vec<String>,
    /// Bumped when membership policy changes; stale tokens are rejected.
    pub policy_version: i64,
    /// Session id.
    pub sid: Uuid,
    /// Token family (refresh rotation family).
    pub family_id: Uuid,
    /// JWT id.
    pub jti: Uuid,
    /// Issuer.
    pub iss: String,
    /// Issued-at (seconds).
    pub iat: i64,
    /// Expiry (seconds).
    pub exp: i64,
}

impl AccessClaims {
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        now.timestamp() >= self.exp
    }
}

#[derive(Debug, Clone)]
pub struct SigningKey {
    pub kid: String,
    pub secret: String,
    pub active: bool,
}

/// In-memory JWKS-style keyring for HS256 rotation.
#[derive(Clone, Default)]
pub struct KeyRing {
    inner: Arc<std::sync::RwLock<KeyRingInner>>,
}

#[derive(Default)]
struct KeyRingInner {
    keys: HashMap<String, SigningKey>,
    active_kid: Option<String>,
}

impl KeyRing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bootstrap from a single env secret (local/dev/CI). Creates kid `bootstrap`.
    pub fn from_secret(secret: impl Into<String>) -> Self {
        let ring = Self::new();
        ring.upsert(SigningKey {
            kid: "bootstrap".into(),
            secret: secret.into(),
            active: true,
        });
        ring
    }

    pub fn upsert(&self, key: SigningKey) {
        let mut g = self.inner.write().expect("keyring");
        if key.active {
            g.active_kid = Some(key.kid.clone());
            for k in g.keys.values_mut() {
                if k.kid != key.kid {
                    k.active = false;
                }
            }
        }
        g.keys.insert(key.kid.clone(), key);
    }

    pub fn retire(&self, kid: &str) {
        let mut g = self.inner.write().expect("keyring");
        if let Some(k) = g.keys.get_mut(kid) {
            k.active = false;
        }
        if g.active_kid.as_deref() == Some(kid) {
            g.active_kid = g.keys.values().find(|k| k.active).map(|k| k.kid.clone());
        }
    }

    pub fn active(&self) -> Result<SigningKey, TokenError> {
        let g = self.inner.read().expect("keyring");
        let kid = g.active_kid.as_ref().ok_or(TokenError::NoActiveKey)?;
        g.keys.get(kid).cloned().ok_or(TokenError::NoActiveKey)
    }

    pub fn get(&self, kid: &str) -> Result<SigningKey, TokenError> {
        let g = self.inner.read().expect("keyring");
        g.keys
            .get(kid)
            .cloned()
            .ok_or_else(|| TokenError::UnknownKid(kid.to_string()))
    }

    /// JWKS-like document (HS256 secrets are base64url-encoded `k` for internal verifiers).
    pub fn jwks_json(&self) -> serde_json::Value {
        let g = self.inner.read().expect("keyring");
        let keys: Vec<_> = g
            .keys
            .values()
            .filter(|k| k.active || g.active_kid.as_deref() == Some(k.kid.as_str()))
            .map(|k| {
                serde_json::json!({
                    "kty": "oct",
                    "alg": "HS256",
                    "kid": k.kid,
                    "k": URL_SAFE_NO_PAD.encode(k.secret.as_bytes()),
                    "use": "sig",
                })
            })
            .collect();
        // Also include recently retired keys for verification grace.
        let retired: Vec<_> = g
            .keys
            .values()
            .filter(|k| !k.active)
            .map(|k| {
                serde_json::json!({
                    "kty": "oct",
                    "alg": "HS256",
                    "kid": k.kid,
                    "k": URL_SAFE_NO_PAD.encode(k.secret.as_bytes()),
                    "use": "sig",
                })
            })
            .collect();
        let mut all = keys;
        for r in retired {
            if !all.iter().any(|x| x["kid"] == r["kid"]) {
                all.push(r);
            }
        }
        serde_json::json!({ "keys": all })
    }
}

pub fn mint_access_token(
    ring: &KeyRing,
    mut claims: AccessClaims,
    ttl: ChronoDuration,
) -> Result<String, TokenError> {
    let key = ring.active()?;
    let now = Utc::now();
    claims.iat = now.timestamp();
    claims.exp = (now + ttl).timestamp();
    if claims.jti.is_nil() {
        claims.jti = Uuid::now_v7();
    }
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(key.kid.clone());
    encode(
        &header,
        &claims,
        &EncodingKey::from_secret(key.secret.as_bytes()),
    )
    .map_err(|e| TokenError::Jwt(e.to_string()))
}

pub fn verify_access_token(ring: &KeyRing, token: &str) -> Result<AccessClaims, TokenError> {
    let header = jsonwebtoken::decode_header(token).map_err(|e| TokenError::Jwt(e.to_string()))?;
    let kid = header
        .kid
        .ok_or_else(|| TokenError::Jwt("missing kid".into()))?;
    let key = ring.get(&kid)?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.set_issuer(&["companyos"]);
    let data = decode::<AccessClaims>(
        token,
        &DecodingKey::from_secret(key.secret.as_bytes()),
        &validation,
    )
    .map_err(|e| TokenError::Jwt(e.to_string()))?;
    if data.claims.org_id.is_empty() || data.claims.org_uuid.is_nil() {
        return Err(TokenError::MissingClaim);
    }
    Ok(data.claims)
}

/// Generate an opaque refresh token (returned to client once; only hash stored).
pub fn generate_opaque_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// SHA-256 hex hash for storing refresh / email tokens.
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn access_ttl() -> ChronoDuration {
    ChronoDuration::seconds(ACCESS_TOKEN_TTL_SECS)
}

pub fn refresh_ttl() -> ChronoDuration {
    ChronoDuration::seconds(REFRESH_TOKEN_TTL_SECS)
}

pub fn refresh_ttl_std() -> Duration {
    Duration::from_secs(REFRESH_TOKEN_TTL_SECS as u64)
}

/// Decode a base64url JWKS `k` field back to secret bytes (utf-8 for our HS256 secrets).
pub fn decode_jwk_k(k: &str) -> Result<Vec<u8>, TokenError> {
    URL_SAFE_NO_PAD
        .decode(k)
        .or_else(|_| STANDARD.decode(k))
        .map_err(|e| TokenError::Jwt(format!("bad jwk k: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_tenancy::OrgId;

    fn sample_claims(org: OrgId, user: Uuid) -> AccessClaims {
        AccessClaims {
            sub: format!("usr_{user}"),
            user_id: user,
            org_id: org.to_public().as_str(),
            org_uuid: org.as_uuid(),
            membership_id: Uuid::now_v7(),
            roles: vec!["owner".into()],
            policy_version: 1,
            sid: Uuid::now_v7(),
            family_id: Uuid::now_v7(),
            jti: Uuid::now_v7(),
            iss: "companyos".into(),
            iat: 0,
            exp: 0,
        }
    }

    #[test]
    fn mint_and_verify_round_trip() {
        let ring = KeyRing::from_secret("test-secret-do-not-use-in-prod");
        let org = OrgId::generate();
        let user = Uuid::now_v7();
        let token = mint_access_token(&ring, sample_claims(org, user), access_ttl()).unwrap();
        let claims = verify_access_token(&ring, &token).unwrap();
        assert_eq!(claims.org_uuid, org.as_uuid());
        assert_eq!(claims.user_id, user);
        assert!(claims.org_id.starts_with("org_"));
    }

    #[test]
    fn org_b_claims_do_not_match_org_a() {
        let ring = KeyRing::from_secret("test-secret");
        let org_a = OrgId::generate();
        let org_b = OrgId::generate();
        let token =
            mint_access_token(&ring, sample_claims(org_a, Uuid::now_v7()), access_ttl()).unwrap();
        let claims = verify_access_token(&ring, &token).unwrap();
        assert_ne!(claims.org_uuid, org_b.as_uuid());
    }

    #[test]
    fn opaque_refresh_hashes_differ() {
        let a = generate_opaque_token();
        let b = generate_opaque_token();
        assert_ne!(a, b);
        assert_ne!(hash_token(&a), hash_token(&b));
        assert_eq!(hash_token(&a), hash_token(&a));
    }

    #[test]
    fn key_rotation_old_kid_still_verifies() {
        let ring = KeyRing::from_secret("old-secret");
        let org = OrgId::generate();
        let token =
            mint_access_token(&ring, sample_claims(org, Uuid::now_v7()), access_ttl()).unwrap();
        ring.upsert(SigningKey {
            kid: "rotated".into(),
            secret: "new-secret".into(),
            active: true,
        });
        // Old token still verifies via retired kid.
        assert!(verify_access_token(&ring, &token).is_ok());
        let new_token =
            mint_access_token(&ring, sample_claims(org, Uuid::now_v7()), access_ttl()).unwrap();
        let header = jsonwebtoken::decode_header(&new_token).unwrap();
        assert_eq!(header.kid.as_deref(), Some("rotated"));
    }
}
