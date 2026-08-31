//! Mock OAuth surface: PKCE authorization codes, token exchange, refresh, and
//! the bearer-token permission check used by downstream services.
//!
//! Code exchange funnels into the same [`create_install`] used by the direct
//! install and connector-connect routes; nothing here is specific to a listing
//! kind.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_errors::AppError;
use companyos_ids::new_uuid_v7;
use companyos_tenancy::{Actor, OrgId};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::install::{create_install, InstallRow};
use super::listings::{fetch_published, ListingRow};
use super::tokens::{issue_tokens, resolve_token, revoke_token_by_id};
use super::types::{
    AuthorizePermissionResponse, OauthTokenRequest, TokenPair, AUTH_CODE_TTL_SECS, TOKEN_ACCESS,
    TOKEN_REFRESH,
};
use super::{
    emit_event, forbidden, internal, org_public, set_org, set_token_lookup, string_array,
    unauthorized, validation,
};

pub const GRANT_AUTHORIZATION_CODE: &str = "authorization_code";
pub const GRANT_REFRESH_TOKEN: &str = "refresh_token";

#[derive(Debug, Clone, FromRow)]
pub struct OauthClientRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub listing_id: Uuid,
    pub public_id: String,
    pub client_id: String,
    pub client_secret_hash: String,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct AuthCodeRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub listing_id: Uuid,
    pub code_challenge: String,
    pub code_challenge_method: String,
    pub redirect_uri: String,
    pub consented_scopes: serde_json::Value,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
}

/// Verify a PKCE code verifier against the stored challenge.
///
/// `S256` is required unless the challenge was recorded as `plain`.
pub fn verify_pkce(challenge: &str, method: &str, verifier: Option<&str>) -> bool {
    if challenge.is_empty() {
        // No challenge recorded — nothing to verify (mock/dev flow).
        return true;
    }
    let Some(verifier) = verifier else {
        return false;
    };
    match method {
        "plain" => challenge == verifier,
        _ => {
            let digest = Sha256::digest(verifier.as_bytes());
            URL_SAFE_NO_PAD.encode(digest) == challenge
        }
    }
}

/// Authenticate an OAuth client by id + secret. Runs under the token-lookup key.
pub async fn authenticate_client(
    tx: &mut Transaction<'_, Postgres>,
    client_id: &str,
    client_secret: &str,
    request_id: &str,
) -> Result<OauthClientRow, AppError> {
    set_token_lookup(tx, request_id).await?;
    let row: Option<OauthClientRow> = sqlx::query_as(
        "SELECT id, org_id, listing_id, public_id, client_id, client_secret_hash, revoked_at \
         FROM marketplace_oauth_client WHERE client_id = $1",
    )
    .bind(client_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let Some(client) = row else {
        return Err(unauthorized(request_id, "invalid client credentials"));
    };
    if client.revoked_at.is_some() {
        return Err(unauthorized(request_id, "client revoked"));
    }
    if client.client_secret_hash != hash_token(client_secret) {
        return Err(unauthorized(request_id, "invalid client credentials"));
    }
    Ok(client)
}

pub struct IssuedCode {
    pub code: String,
    pub expires_at: DateTime<Utc>,
}

/// Consent captured at the authorize step, ready to be minted into a code.
pub struct CodeGrant<'a> {
    pub listing: &'a ListingRow,
    pub consented: &'a [String],
    pub redirect_uri: &'a str,
    pub code_challenge: &'a str,
    pub code_challenge_method: &'a str,
    pub created_by: Uuid,
}

/// Record a single-use authorization code carrying the granted consent.
pub async fn create_authorization_code(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    grant: &CodeGrant<'_>,
    request_id: &str,
) -> Result<IssuedCode, AppError> {
    let CodeGrant {
        listing,
        consented,
        redirect_uri,
        code_challenge,
        code_challenge_method,
        created_by,
    } = *grant;

    let allowed = listing.redirect_uris();
    if !allowed.is_empty() && !allowed.iter().any(|u| u == redirect_uri) {
        return Err(validation(
            request_id,
            "redirect_uri is not registered for this listing",
        ));
    }
    if !matches!(code_challenge_method, "S256" | "plain") {
        return Err(validation(
            request_id,
            "code_challenge_method must be S256 or plain",
        ));
    }

    let code = format!("mac_{}", generate_opaque_token());
    let expires_at = Utc::now() + Duration::seconds(AUTH_CODE_TTL_SECS);
    sqlx::query(
        r#"
        INSERT INTO marketplace_oauth_code (
            id, org_id, listing_id, code_hash, code_challenge, code_challenge_method,
            redirect_uri, consented_scopes, expires_at, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id.as_uuid())
    .bind(listing.id)
    .bind(hash_token(&code))
    .bind(code_challenge)
    .bind(code_challenge_method)
    .bind(redirect_uri)
    .bind(json!(consented))
    .bind(expires_at)
    .bind(created_by)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok(IssuedCode { code, expires_at })
}

/// Exchange an authorization code for an install plus its first token pair.
pub async fn exchange_authorization_code(
    pool: &sqlx::PgPool,
    req: &OauthTokenRequest,
    request_id: &str,
) -> Result<(InstallRow, TokenPair), AppError> {
    let Some(code) = req.code.as_deref() else {
        return Err(validation(request_id, "code is required"));
    };

    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    let client = authenticate_client(&mut tx, &req.client_id, &req.client_secret, request_id).await?;

    let row: Option<AuthCodeRow> = sqlx::query_as(
        "SELECT id, org_id, listing_id, code_challenge, code_challenge_method, redirect_uri, \
         consented_scopes, expires_at, used_at, created_by \
         FROM marketplace_oauth_code WHERE code_hash = $1",
    )
    .bind(hash_token(code))
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?;

    let Some(auth_code) = row else {
        return Err(unauthorized(request_id, "authorization code not recognised"));
    };
    if auth_code.used_at.is_some() {
        return Err(unauthorized(request_id, "authorization code already used"));
    }
    if auth_code.expires_at <= Utc::now() {
        return Err(unauthorized(request_id, "authorization code expired"));
    }
    if auth_code.listing_id != client.listing_id {
        return Err(unauthorized(
            request_id,
            "authorization code was issued for a different client",
        ));
    }
    if let Some(redirect_uri) = req.redirect_uri.as_deref() {
        if redirect_uri != auth_code.redirect_uri {
            return Err(unauthorized(request_id, "redirect_uri mismatch"));
        }
    }
    if !verify_pkce(
        &auth_code.code_challenge,
        &auth_code.code_challenge_method,
        req.code_verifier.as_deref(),
    ) {
        return Err(unauthorized(request_id, "PKCE verification failed"));
    }

    let org_id = OrgId::new(auth_code.org_id);
    set_org(&mut tx, org_id, request_id).await?;

    let listing = fetch_published(&mut tx, auth_code.listing_id, request_id).await?;
    let consented = string_array(&auth_code.consented_scopes);
    let actor = Actor::human(auth_code.created_by);

    let (install, tokens) = create_install(
        &mut tx,
        org_id,
        auth_code.created_by,
        actor,
        &listing,
        &consented,
        request_id,
    )
    .await?;

    sqlx::query(
        "UPDATE marketplace_oauth_code SET used_at = now(), install_id = $3 \
         WHERE org_id = $1 AND id = $2",
    )
    .bind(auth_code.org_id)
    .bind(auth_code.id)
    .bind(install.id)
    .execute(&mut *tx)
    .await
    .map_err(internal(request_id))?;

    tx.commit().await.map_err(internal(request_id))?;
    Ok((install, tokens))
}

/// Rotate a refresh token. Scopes stay pinned to the install's consent.
pub async fn exchange_refresh_token(
    pool: &sqlx::PgPool,
    req: &OauthTokenRequest,
    request_id: &str,
) -> Result<(InstallRow, TokenPair), AppError> {
    let Some(refresh) = req.refresh_token.as_deref() else {
        return Err(validation(request_id, "refresh_token is required"));
    };

    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    let client = authenticate_client(&mut tx, &req.client_id, &req.client_secret, request_id).await?;
    let resolved = resolve_token(&mut tx, refresh, TOKEN_REFRESH, request_id).await?;
    if resolved.listing_id != client.listing_id {
        return Err(unauthorized(
            request_id,
            "refresh token belongs to a different client",
        ));
    }

    let org_id = OrgId::new(resolved.org_id);
    revoke_token_by_id(&mut tx, org_id, resolved.token.id, request_id).await?;
    let tokens = issue_tokens(
        &mut tx,
        org_id,
        resolved.install_id,
        &resolved.consented_scopes,
        request_id,
    )
    .await?;
    let install = super::install::fetch(&mut tx, org_id, resolved.install_id, request_id).await?;

    emit_event(
        &mut tx,
        org_id,
        Actor::human(install.installed_by),
        "oauth_token_issued",
        json!({
            "install_id": install.public_id,
            "listing_id": install.listing_public_id,
            "scopes": resolved.consented_scopes,
            "reason": "refresh",
        }),
        request_id,
    )
    .await?;

    tx.commit().await.map_err(internal(request_id))?;
    Ok((install, tokens))
}

/// Bearer-token permission check used by resource servers.
///
/// 401 when the token is unknown, revoked, expired or its install is not
/// active; 403 when the permission was never consented to.
pub async fn authorize_permission(
    pool: &sqlx::PgPool,
    access_token: &str,
    permission: &str,
    request_id: &str,
) -> Result<AuthorizePermissionResponse, AppError> {
    if permission.trim().is_empty() {
        return Err(validation(request_id, "permission is required"));
    }
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    let resolved = resolve_token(&mut tx, access_token, TOKEN_ACCESS, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;

    if !resolved.scopes.iter().any(|s| s == permission) {
        return Err(forbidden(
            request_id,
            format!("token was not granted {permission}"),
        ));
    }

    Ok(AuthorizePermissionResponse {
        allowed: true,
        install_id: resolved.install_public_id,
        org_id: org_public(resolved.org_id),
        permission: permission.to_string(),
        scopes: resolved.scopes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s256_pkce_round_trip() {
        let verifier = "a-high-entropy-code-verifier-value-1234567890";
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert!(verify_pkce(&challenge, "S256", Some(verifier)));
        assert!(!verify_pkce(&challenge, "S256", Some("wrong-verifier")));
        assert!(!verify_pkce(&challenge, "S256", None));
    }

    #[test]
    fn plain_pkce_and_empty_challenge() {
        assert!(verify_pkce("abc", "plain", Some("abc")));
        assert!(!verify_pkce("abc", "plain", Some("abd")));
        assert!(verify_pkce("", "S256", None));
    }
}
