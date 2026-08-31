//! Marketplace app tokens.
//!
//! Tokens are opaque and stored only as SHA-256 hashes. The scopes recorded on
//! a token are a snapshot of the install's consented scopes at issue time —
//! widening consent must revoke and re-issue, never mutate a live token.

use chrono::{DateTime, Duration, Utc};
use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::OrgId;
use serde_json::json;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::types::{
    TokenPair, ACCESS_TOKEN_TTL_SECS, INSTALL_ACTIVE, REFRESH_TOKEN_TTL_SECS, TOKEN_ACCESS,
    TOKEN_REFRESH,
};
use super::{internal, set_org, set_token_lookup, string_array, unauthorized};

const ACCESS_PREFIX: &str = "mat_";
const REFRESH_PREFIX: &str = "mrt_";

#[derive(Debug, Clone, FromRow)]
pub struct AppTokenRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub install_id: Uuid,
    pub public_id: String,
    pub token_kind: String,
    pub scopes: serde_json::Value,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

const TOKEN_COLUMNS: &str =
    "id, org_id, install_id, public_id, token_kind, scopes, expires_at, revoked_at";

/// A token resolved from its plaintext, together with its install's live state.
#[derive(Debug, Clone)]
pub struct ResolvedToken {
    pub token: AppTokenRow,
    pub install_id: Uuid,
    pub install_public_id: String,
    pub org_id: Uuid,
    pub install_status: String,
    pub listing_id: Uuid,
    pub consented_scopes: Vec<String>,
    pub scopes: Vec<String>,
}

fn token_prefix(token: &str) -> String {
    token.chars().take(12).collect()
}

async fn insert_token(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    install_id: Uuid,
    kind: &str,
    plaintext: &str,
    scopes: &[String],
    expires_at: DateTime<Utc>,
    request_id: &str,
) -> Result<String, AppError> {
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::MarketplaceAppToken, id).as_str();
    sqlx::query(
        r#"
        INSERT INTO marketplace_app_token (
            id, org_id, install_id, public_id, token_kind, token_hash, token_prefix,
            scopes, expires_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(install_id)
    .bind(&public_id)
    .bind(kind)
    .bind(hash_token(plaintext))
    .bind(token_prefix(plaintext))
    .bind(json!(scopes))
    .bind(expires_at)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(public_id)
}

/// Issue an access + refresh pair for an install. Plaintext is returned once.
///
/// Shared by direct installs, connector connects and OAuth code/refresh
/// exchanges — there is no per-listing-kind variant.
pub async fn issue_tokens(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    install_id: Uuid,
    scopes: &[String],
    request_id: &str,
) -> Result<TokenPair, AppError> {
    let now = Utc::now();
    let access = format!("{ACCESS_PREFIX}{}", generate_opaque_token());
    let refresh = format!("{REFRESH_PREFIX}{}", generate_opaque_token());

    insert_token(
        tx,
        org_id,
        install_id,
        TOKEN_ACCESS,
        &access,
        scopes,
        now + Duration::seconds(ACCESS_TOKEN_TTL_SECS),
        request_id,
    )
    .await?;
    insert_token(
        tx,
        org_id,
        install_id,
        TOKEN_REFRESH,
        &refresh,
        scopes,
        now + Duration::seconds(REFRESH_TOKEN_TTL_SECS),
        request_id,
    )
    .await?;

    Ok(TokenPair {
        access_token: access,
        refresh_token: refresh,
        token_type: "Bearer",
        expires_in: ACCESS_TOKEN_TTL_SECS,
        scope: scopes.to_vec(),
    })
}

/// Revoke every live token (access **and** refresh) for an install.
pub async fn revoke_install_tokens(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    install_id: Uuid,
    request_id: &str,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE marketplace_app_token SET revoked_at = now() \
         WHERE org_id = $1 AND install_id = $2 AND revoked_at IS NULL",
    )
    .bind(org_id.as_uuid())
    .bind(install_id)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(result.rows_affected())
}

/// Resolve an opaque token to its install.
///
/// The token hash lookup runs under the `app.marketplace_token_lookup` RLS key
/// (the caller has no org context yet); the install read then re-binds
/// `app.org_id` to the org recorded on the token, so the install itself is
/// still read under strict tenant isolation.
pub async fn resolve_token(
    tx: &mut Transaction<'_, Postgres>,
    plaintext: &str,
    expected_kind: &str,
    request_id: &str,
) -> Result<ResolvedToken, AppError> {
    set_token_lookup(tx, request_id).await?;
    let token: Option<AppTokenRow> = sqlx::query_as(&format!(
        "SELECT {TOKEN_COLUMNS} FROM marketplace_app_token WHERE token_hash = $1"
    ))
    .bind(hash_token(plaintext))
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let Some(token) = token else {
        return Err(unauthorized(request_id, "token not recognised"));
    };
    if token.token_kind != expected_kind {
        return Err(unauthorized(request_id, "token kind mismatch"));
    }
    if token.revoked_at.is_some() {
        return Err(unauthorized(request_id, "token revoked"));
    }
    if token.expires_at.is_some_and(|exp| exp <= Utc::now()) {
        return Err(unauthorized(request_id, "token expired"));
    }

    let org_id = OrgId::new(token.org_id);
    set_org(tx, org_id, request_id).await?;

    #[allow(clippy::type_complexity)]
    let install: Option<(String, String, Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT public_id, status, listing_id, consented_scopes \
         FROM marketplace_install WHERE org_id = $1 AND id = $2",
    )
    .bind(token.org_id)
    .bind(token.install_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let Some((install_public_id, install_status, listing_id, consented)) = install else {
        return Err(unauthorized(request_id, "install not found for token"));
    };
    if install_status != INSTALL_ACTIVE {
        return Err(unauthorized(request_id, "install is not active"));
    }

    let scopes = string_array(&token.scopes);
    Ok(ResolvedToken {
        install_id: token.install_id,
        install_public_id,
        org_id: token.org_id,
        install_status,
        listing_id,
        consented_scopes: string_array(&consented),
        scopes,
        token,
    })
}

pub async fn revoke_token_by_id(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    token_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE marketplace_app_token SET revoked_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id.as_uuid())
    .bind(token_id)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_tokens_are_prefixed_and_hash_stably() {
        let raw = format!("{ACCESS_PREFIX}{}", generate_opaque_token());
        assert!(raw.starts_with("mat_"));
        assert_eq!(token_prefix(&raw).len(), 12);
        assert_eq!(hash_token(&raw), hash_token(&raw));
        assert_ne!(hash_token(&raw), raw);
    }
}
