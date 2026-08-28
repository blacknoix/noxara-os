//! Phase 1.1 Identity & Authentication.
//!
//! Primary path: signed org-scoped access JWTs + opaque refresh cookies.
//! LOCAL-ONLY header/unsigned bearer is available only when
//! `COMPANYOS_LOCAL_AUTH=1` (default **off**).

mod audit;
mod breach;
pub mod extract;
pub mod handlers;
pub mod lockout;
pub mod mail;
pub mod mfa;
pub mod oauth;
pub mod password;
pub mod rate_limit;
pub mod sessions;
pub mod sso;
pub mod sso_login;
pub mod tokens;

pub use handlers::router;

use companyos_auth_token::KeyRing;
use sqlx::PgPool;

use crate::state::AppState;

/// Build auth-related application state pieces.
pub fn build_keyring() -> KeyRing {
    let secret = std::env::var("AUTH_JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!("AUTH_JWT_SECRET unset — using ephemeral in-process secret");
        format!("dev-only-{}", uuid::Uuid::now_v7())
    });
    KeyRing::from_secret(secret)
}

pub fn local_auth_enabled() -> bool {
    matches!(
        std::env::var("COMPANYOS_LOCAL_AUTH").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

/// SSO feature is plan-gated; global kill-switch defaults to disabled.
pub fn sso_globally_enabled() -> bool {
    matches!(
        std::env::var("COMPANYOS_SSO_ENABLED").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

pub async fn ensure_bootstrap_key(pool: &PgPool, ring: &KeyRing) -> anyhow::Result<()> {
    let active = ring.active()?;
    sqlx::query(
        r#"
        INSERT INTO jwks_signing_key (kid, algorithm, secret_material, is_active)
        VALUES ($1, 'HS256', $2, true)
        ON CONFLICT (kid) DO UPDATE SET secret_material = EXCLUDED.secret_material, is_active = true
        "#,
    )
    .bind(&active.kid)
    .bind(&active.secret)
    .execute(pool)
    .await?;
    // Load any additional keys from DB (rotation).
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

#[allow(dead_code)]
pub fn attach(_state: &AppState) {}
