//! Google + Microsoft OAuth (authorization code + PKCE). Credentials from env.
//! Tests mock the token/userinfo HTTP endpoints via `OAUTH_MOCK_BASE`.

use base64::Engine;
use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_ids::new_uuid_v7;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Google,
    Microsoft,
}

impl OAuthProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "google" => Some(Self::Google),
            "microsoft" => Some(Self::Microsoft),
            _ => None,
        }
    }

    pub fn authorize_url(self) -> String {
        if let Ok(base) = std::env::var("OAUTH_MOCK_BASE") {
            return format!("{}/{}/authorize", base.trim_end_matches('/'), self.as_str());
        }
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth".into(),
            Self::Microsoft => {
                "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into()
            }
        }
    }

    pub fn token_url(self) -> String {
        if let Ok(base) = std::env::var("OAUTH_MOCK_BASE") {
            return format!("{}/{}/token", base.trim_end_matches('/'), self.as_str());
        }
        match self {
            Self::Google => "https://oauth2.googleapis.com/token".into(),
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
        }
    }

    pub fn userinfo_url(self) -> String {
        if let Ok(base) = std::env::var("OAUTH_MOCK_BASE") {
            return format!("{}/{}/userinfo", base.trim_end_matches('/'), self.as_str());
        }
        match self {
            Self::Google => "https://openidconnect.googleapis.com/v1/userinfo".into(),
            Self::Microsoft => "https://graph.microsoft.com/oidc/userinfo".into(),
        }
    }

    pub fn client_id(self) -> Option<String> {
        match self {
            Self::Google => std::env::var("GOOGLE_OAUTH_CLIENT_ID").ok(),
            Self::Microsoft => std::env::var("MICROSOFT_OAUTH_CLIENT_ID").ok(),
        }
        .filter(|s| !s.is_empty())
    }

    pub fn client_secret(self) -> Option<String> {
        match self {
            Self::Google => std::env::var("GOOGLE_OAUTH_CLIENT_SECRET").ok(),
            Self::Microsoft => std::env::var("MICROSOFT_OAUTH_CLIENT_SECRET").ok(),
        }
        .filter(|s| !s.is_empty())
    }

    pub fn scopes(self) -> &'static str {
        match self {
            Self::Google => "openid email profile",
            Self::Microsoft => "openid email profile",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct OAuthUserInfo {
    pub sub: String,
    pub email: Option<String>,
    #[allow(dead_code)]
    pub email_verified: Option<bool>,
    pub name: Option<String>,
}

pub fn pkce_pair() -> (String, String) {
    let verifier = generate_opaque_token();
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());
    (verifier, challenge)
}

pub async fn store_state(
    pool: &PgPool,
    provider: OAuthProvider,
    state: &str,
    code_verifier: &str,
    redirect_uri: &str,
    nonce: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO oauth_state (id, provider, state_hash, code_verifier, redirect_uri, nonce, expires_at)
        VALUES ($1,$2,$3,$4,$5,$6, now() + interval '10 minutes')
        "#,
    )
    .bind(id)
    .bind(provider.as_str())
    .bind(hash_token(state))
    .bind(code_verifier)
    .bind(redirect_uri)
    .bind(nonce)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn take_state(
    pool: &PgPool,
    state: &str,
) -> Result<Option<(String, String, String, String)>, sqlx::Error> {
    let row: Option<(String, String, String, String, Uuid)> = sqlx::query_as(
        r#"
        SELECT provider, code_verifier, redirect_uri, nonce, id
        FROM oauth_state
        WHERE state_hash = $1 AND expires_at > now()
        "#,
    )
    .bind(hash_token(state))
    .fetch_optional(pool)
    .await?;
    if let Some((provider, verifier, redirect, nonce, id)) = row {
        sqlx::query("DELETE FROM oauth_state WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(Some((provider, verifier, redirect, nonce)))
    } else {
        Ok(None)
    }
}

pub async fn exchange_code(
    provider: OAuthProvider,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<String, String> {
    let client_id = provider
        .client_id()
        .ok_or_else(|| format!("{} client id not configured", provider.as_str()))?;
    let client_secret = provider.client_secret().unwrap_or_default();
    let client = reqwest::Client::new();
    let resp = client
        .post(provider.token_url())
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("token exchange failed: {body}"));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    json.get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "token response missing access_token".into())
}

pub async fn fetch_userinfo(
    provider: OAuthProvider,
    access_token: &str,
) -> Result<OAuthUserInfo, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(provider.userinfo_url())
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    resp.json().await.map_err(|e| e.to_string())
}
