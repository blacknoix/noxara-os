//! Enterprise OIDC SSO login (Phase 2.6).
//!
//! Config CRUD remains in `sso.rs`. This module owns start/callback + mocked IdP
//! support via `SSO_MOCK_BASE`. Membership must already exist — no god-account
//! auto-provisioning.

use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_outbox::insert_event;
use companyos_tenancy::{set_session_org_id, set_sso_lookup, Actor, OrgId};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use super::oauth::{self, OAuthUserInfo};
use super::sessions::{self, IssuedTokens};
use super::{audit, sso};
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct OidcEndpoints {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
    pub client_id: String,
    pub client_secret: String,
    pub idp_key: String,
}

pub fn endpoints_from_config(config: &serde_json::Value) -> Result<OidcEndpoints, String> {
    let idp_key = config
        .get("idp_key")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let client_id = config
        .get("client_id")
        .and_then(|v| v.as_str())
        .ok_or("config.client_id required")?
        .to_string();
    let client_secret = config
        .get("client_secret")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if let Ok(base) = std::env::var("SSO_MOCK_BASE") {
        let base = base.trim_end_matches('/');
        return Ok(OidcEndpoints {
            authorization_endpoint: format!("{base}/{idp_key}/authorize"),
            token_endpoint: format!("{base}/{idp_key}/token"),
            userinfo_endpoint: format!("{base}/{idp_key}/userinfo"),
            client_id,
            client_secret,
            idp_key,
        });
    }

    Ok(OidcEndpoints {
        authorization_endpoint: config
            .get("authorization_endpoint")
            .and_then(|v| v.as_str())
            .ok_or("config.authorization_endpoint required")?
            .to_string(),
        token_endpoint: config
            .get("token_endpoint")
            .and_then(|v| v.as_str())
            .ok_or("config.token_endpoint required")?
            .to_string(),
        userinfo_endpoint: config
            .get("userinfo_endpoint")
            .and_then(|v| v.as_str())
            .ok_or("config.userinfo_endpoint required")?
            .to_string(),
        client_id,
        client_secret,
        idp_key,
    })
}

async fn enable_sso_lookup(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), sqlx::Error> {
    set_sso_lookup(tx)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))
}

pub async fn find_config_by_public_id(
    pool: &PgPool,
    public_id: &str,
) -> Result<Option<(Uuid, Uuid, String, serde_json::Value, bool)>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    enable_sso_lookup(&mut tx).await?;
    let row: Option<(Uuid, Uuid, String, serde_json::Value, bool)> = sqlx::query_as(
        r#"
        SELECT id, org_id, protocol, config, enabled
        FROM sso_configuration
        WHERE public_id = $1
        "#,
    )
    .bind(public_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn start_oidc_login(
    pool: &PgPool,
    config_public_id: &str,
    redirect_uri: &str,
    request_id: &str,
) -> Result<String, AppError> {
    let Some((config_id, org_uuid, protocol, config, enabled)) =
        find_config_by_public_id(pool, config_public_id)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?
    else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "sso config not found",
        ));
    };
    if protocol != "oidc" {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "only oidc login is supported in Phase 2.6",
        ));
    }
    if !enabled {
        return Err(AppError::new(
            ErrorCode::FeatureDisabled,
            request_id,
            "sso configuration is disabled",
        ));
    }
    sso::require_sso_feature(pool, org_uuid, request_id).await?;

    let endpoints = endpoints_from_config(&config)
        .map_err(|e| AppError::new(ErrorCode::ValidationFailed, request_id, e))?;

    let state_token = generate_opaque_token();
    let (verifier, challenge) = oauth::pkce_pair();
    let nonce = generate_opaque_token();
    let state_hash = hash_token(&state_token);

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, OrgId::new(org_uuid))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    sqlx::query(
        r#"
        INSERT INTO sso_login_state (
            id, org_id, sso_config_id, state_hash, code_verifier, nonce, redirect_uri, expires_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7, now() + interval '10 minutes')
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_uuid)
    .bind(config_id)
    .bind(&state_hash)
    .bind(&verifier)
    .bind(&nonce)
    .bind(redirect_uri)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&nonce={}",
        endpoints.authorization_endpoint,
        urlencoding::encode(&endpoints.client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode("openid email profile"),
        urlencoding::encode(&state_token),
        urlencoding::encode(&challenge),
        urlencoding::encode(&nonce),
    ))
}

#[derive(Debug, Deserialize)]
struct TokenResponseBody {
    access_token: String,
}

pub async fn exchange_code_for_userinfo(
    endpoints: &OidcEndpoints,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthUserInfo, String> {
    let client = reqwest::Client::new();
    let token_res = client
        .post(&endpoints.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", endpoints.client_id.as_str()),
            ("client_secret", endpoints.client_secret.as_str()),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !token_res.status().is_success() {
        return Err(format!("token endpoint status {}", token_res.status()));
    }
    let token: TokenResponseBody = token_res.json().await.map_err(|e| e.to_string())?;
    let info_res = client
        .get(&endpoints.userinfo_endpoint)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !info_res.status().is_success() {
        return Err(format!("userinfo endpoint status {}", info_res.status()));
    }
    info_res.json().await.map_err(|e| e.to_string())
}

#[derive(Debug, sqlx::FromRow)]
struct SsoLoginStateRow {
    id: Uuid,
    org_id: Uuid,
    sso_config_id: Uuid,
    code_verifier: String,
    nonce: String,
    redirect_uri: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

pub async fn complete_oidc_login(
    state: &AppState,
    state_token: &str,
    code: &str,
    request_id: &str,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<IssuedTokens, AppError> {
    let state_hash = hash_token(state_token);
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    enable_sso_lookup(&mut tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let row: Option<SsoLoginStateRow> = sqlx::query_as(
        r#"
        SELECT id, org_id, sso_config_id, code_verifier, nonce, redirect_uri, expires_at
        FROM sso_login_state
        WHERE state_hash = $1
        "#,
    )
    .bind(&state_hash)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let Some(row) = row else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid sso state",
        ));
    };
    let state_id = row.id;
    let org_uuid = row.org_id;
    let config_id = row.sso_config_id;
    let verifier = row.code_verifier;
    let redirect_uri = row.redirect_uri;
    let expires_at = row.expires_at;
    let _nonce = row.nonce;
    if expires_at < chrono::Utc::now() {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "sso state expired",
        ));
    }
    sqlx::query("DELETE FROM sso_login_state WHERE id = $1")
        .bind(state_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let cfg: (String, serde_json::Value, bool) =
        sqlx::query_as("SELECT protocol, config, enabled FROM sso_configuration WHERE id = $1")
            .bind(config_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    if cfg.0 != "oidc" || !cfg.2 {
        return Err(AppError::new(
            ErrorCode::FeatureDisabled,
            request_id,
            "sso configuration unavailable",
        ));
    }
    sso::require_sso_feature(&state.pool, org_uuid, request_id).await?;
    let endpoints = endpoints_from_config(&cfg.1)
        .map_err(|e| AppError::new(ErrorCode::ValidationFailed, request_id, e))?;
    let userinfo = exchange_code_for_userinfo(&endpoints, code, &verifier, &redirect_uri)
        .await
        .map_err(|e| AppError::new(ErrorCode::Unauthorized, request_id, e))?;

    let email = userinfo
        .email
        .as_deref()
        .ok_or_else(|| AppError::new(ErrorCode::Unauthorized, request_id, "IdP email required"))?;
    let email_norm = email.trim().to_lowercase();

    let user: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, public_id FROM user_identity WHERE email_normalized = $1")
            .bind(&email_norm)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let Some((user_id, user_public)) = user else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "no local user for IdP identity; invite the user first",
        ));
    };

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, OrgId::new(org_uuid))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let membership: Option<(Uuid, String, i64)> = sqlx::query_as(
        r#"
        SELECT id, role, policy_version
        FROM membership
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL AND status = 'active'
        "#,
    )
    .bind(org_uuid)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let Some((membership_id, role, policy_version)) = membership else {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "user is not an active member of this organization",
        ));
    };

    sqlx::query(
        r#"
        INSERT INTO sso_identity_link (id, org_id, sso_config_id, user_id, idp_subject, email)
        VALUES ($1,$2,$3,$4,$5,$6)
        ON CONFLICT (org_id, sso_config_id, idp_subject) DO UPDATE
          SET user_id = EXCLUDED.user_id, email = EXCLUDED.email
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_uuid)
    .bind(config_id)
    .bind(user_id)
    .bind(&userinfo.sub)
    .bind(email)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let org = OrgId::new(org_uuid);
    let actor = Actor::human(user_id);
    let mut envelope = EventEnvelope::new(
        org,
        Context::Auth,
        "sso",
        "linked",
        1,
        actor,
        serde_json::json!({
            "sso_config_id": PublicId::new(IdKind::SsoConfig, config_id).as_str(),
            "idp_key": endpoints.idp_key,
            "idp_subject": userinfo.sub,
        }),
    );
    envelope.idempotency_key = format!("auth.sso.linked:{config_id}:{}", userinfo.sub);
    insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let roles = vec![role];
    let issued = sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        user_id,
        &user_public,
        org,
        membership_id,
        &roles,
        policy_version,
        Some("sso"),
        user_agent,
        ip,
    )
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    audit::record(
        &state.pool,
        Some(org_uuid),
        Some(user_id),
        "sso.login",
        ip,
        user_agent,
        serde_json::json!({ "idp_key": endpoints.idp_key, "protocol": "oidc" }),
    )
    .await;

    Ok(issued)
}
