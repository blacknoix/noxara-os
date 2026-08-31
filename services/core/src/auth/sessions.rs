//! Session + refresh token rotation with reuse detection.

use chrono::{Duration, Utc};
use companyos_auth_token::{
    generate_opaque_token, hash_token, refresh_ttl, AccessClaims, KeyRing, REFRESH_COOKIE_NAME,
};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::OrgId;
use cookie::{Cookie, SameSite};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionView {
    pub id: String,
    pub org_id: String,
    pub device_label: Option<String>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: String,
    pub last_seen_at: String,
    pub current: bool,
}

#[derive(Debug)]
pub struct IssuedTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub session_id: Uuid,
    #[allow(dead_code)]
    pub family_id: Uuid,
}

#[derive(Debug)]
pub enum RefreshOutcome {
    Issued(IssuedTokens),
    /// Reuse of a rotated token — entire family revoked.
    ReuseDetected {
        family_id: Uuid,
    },
}

#[allow(clippy::too_many_arguments)]
pub async fn create_session_with_tokens(
    tx: &mut Transaction<'_, Postgres>,
    ring: &KeyRing,
    user_id: Uuid,
    user_public: &str,
    org: OrgId,
    membership_id: Uuid,
    roles: &[String],
    policy_version: i64,
    device_label: Option<&str>,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> Result<IssuedTokens, String> {
    let session_id = new_uuid_v7();
    let family_id = new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO auth_session (
            id, family_id, user_id, org_id, membership_id,
            device_label, user_agent, ip_address
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        "#,
    )
    .bind(session_id)
    .bind(family_id)
    .bind(user_id)
    .bind(org.as_uuid())
    .bind(membership_id)
    .bind(device_label)
    .bind(user_agent)
    .bind(ip)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    issue_refresh_and_access(
        tx,
        ring,
        session_id,
        family_id,
        user_id,
        user_public,
        org,
        membership_id,
        roles,
        policy_version,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn issue_refresh_and_access(
    tx: &mut Transaction<'_, Postgres>,
    ring: &KeyRing,
    session_id: Uuid,
    family_id: Uuid,
    user_id: Uuid,
    user_public: &str,
    org: OrgId,
    membership_id: Uuid,
    roles: &[String],
    policy_version: i64,
) -> Result<IssuedTokens, String> {
    let refresh = generate_opaque_token();
    let refresh_id = new_uuid_v7();
    let expires_at = Utc::now() + refresh_ttl();
    sqlx::query(
        r#"
        INSERT INTO refresh_token (id, session_id, family_id, token_hash, expires_at)
        VALUES ($1,$2,$3,$4,$5)
        "#,
    )
    .bind(refresh_id)
    .bind(session_id)
    .bind(family_id)
    .bind(hash_token(&refresh))
    .bind(expires_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    let claims = AccessClaims {
        sub: user_public.to_string(),
        user_id,
        org_id: org.to_public().as_str(),
        org_uuid: org.as_uuid(),
        membership_id,
        roles: roles.to_vec(),
        policy_version,
        sid: session_id,
        family_id,
        jti: new_uuid_v7(),
        iss: "companyos".into(),
        iat: 0,
        exp: 0,
        api_key_id: None,
        scopes: None,
    };
    let access =
        companyos_auth_token::mint_access_token(ring, claims, companyos_auth_token::access_ttl())
            .map_err(|e| e.to_string())?;

    Ok(IssuedTokens {
        access_token: access,
        refresh_token: refresh,
        expires_in: companyos_auth_token::ACCESS_TOKEN_TTL_SECS,
        session_id,
        family_id,
    })
}

pub async fn rotate_refresh(
    pool: &PgPool,
    ring: &KeyRing,
    presented: &str,
) -> Result<RefreshOutcome, String> {
    let hash = hash_token(presented);
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    #[allow(clippy::type_complexity)]
    let row: Option<(
        Uuid,
        Uuid,
        Uuid,
        Option<chrono::DateTime<Utc>>,
        Option<chrono::DateTime<Utc>>,
        chrono::DateTime<Utc>,
    )> = sqlx::query_as(
        r#"
            SELECT id, session_id, family_id, rotated_at, revoked_at, expires_at
            FROM refresh_token
            WHERE token_hash = $1
            FOR UPDATE
            "#,
    )
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let Some((token_id, session_id, family_id, rotated_at, revoked_at, expires_at)) = row else {
        return Err("invalid refresh token".into());
    };

    if revoked_at.is_some() || expires_at < Utc::now() {
        return Err("refresh token revoked or expired".into());
    }

    // Reuse detection: presented token was already rotated.
    if rotated_at.is_some() {
        revoke_family(&mut tx, family_id, "refresh_reuse_detected")
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        return Ok(RefreshOutcome::ReuseDetected { family_id });
    }

    let session: (Uuid, Uuid, Uuid, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        r#"
        SELECT user_id, org_id, membership_id, revoked_at
        FROM auth_session WHERE id = $1 FOR UPDATE
        "#,
    )
    .bind(session_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    if session.3.is_some() {
        return Err("session revoked".into());
    }

    let (user_id, org_uuid, membership_id) = (session.0, session.1, session.2);
    companyos_tenancy::set_session_org_id(&mut tx, OrgId::new(org_uuid))
        .await
        .map_err(|e| e.to_string())?;
    let membership: (String, i64, Option<chrono::DateTime<Utc>>, String, String) = sqlx::query_as(
        r#"
        SELECT m.role, m.policy_version, m.revoked_at, u.public_id, m.status
        FROM membership m
        JOIN user_identity u ON u.id = m.user_id
        WHERE m.id = $1
        "#,
    )
    .bind(membership_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    if membership.2.is_some() || membership.4 == "revoked" || membership.4 == "suspended" {
        revoke_family(&mut tx, family_id, "membership_revoked")
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        return Err("membership revoked".into());
    }

    let new_refresh = generate_opaque_token();
    let new_id = new_uuid_v7();
    let new_exp = Utc::now() + refresh_ttl();
    sqlx::query(
        r#"
        INSERT INTO refresh_token (id, session_id, family_id, token_hash, expires_at)
        VALUES ($1,$2,$3,$4,$5)
        "#,
    )
    .bind(new_id)
    .bind(session_id)
    .bind(family_id)
    .bind(hash_token(&new_refresh))
    .bind(new_exp)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE refresh_token
        SET rotated_at = now(), replaced_by = $2
        WHERE id = $1
        "#,
    )
    .bind(token_id)
    .bind(new_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query("UPDATE auth_session SET last_seen_at = now() WHERE id = $1")
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    let org = OrgId::new(org_uuid);
    let roles = vec![membership.0.clone()];
    let claims = AccessClaims {
        sub: membership.3.clone(),
        user_id,
        org_id: org.to_public().as_str(),
        org_uuid,
        membership_id,
        roles: roles.clone(),
        policy_version: membership.1,
        sid: session_id,
        family_id,
        jti: new_uuid_v7(),
        iss: "companyos".into(),
        iat: 0,
        exp: 0,
        api_key_id: None,
        scopes: None,
    };
    let access =
        companyos_auth_token::mint_access_token(ring, claims, companyos_auth_token::access_ttl())
            .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;

    Ok(RefreshOutcome::Issued(IssuedTokens {
        access_token: access,
        refresh_token: new_refresh,
        expires_in: companyos_auth_token::ACCESS_TOKEN_TTL_SECS,
        session_id,
        family_id,
    }))
}

pub async fn revoke_family(
    tx: &mut Transaction<'_, Postgres>,
    family_id: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE refresh_token SET revoked_at = now()
        WHERE family_id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(family_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        r#"
        UPDATE auth_session SET revoked_at = now(), revoke_reason = $2
        WHERE family_id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(family_id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn revoke_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT family_id FROM auth_session WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((family_id,)) = row else {
        return Ok(false);
    };
    revoke_family(&mut tx, family_id, "user_revoke_session").await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn revoke_all_sessions(pool: &PgPool, user_id: Uuid) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let families: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT DISTINCT family_id FROM auth_session WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    for (family_id,) in &families {
        revoke_family(&mut tx, *family_id, "user_revoke_all").await?;
    }
    tx.commit().await?;
    Ok(families.len() as u64)
}

/// Revoke all sessions for a user within an org (membership revocation).
pub async fn revoke_org_user_sessions(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let families: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT family_id FROM auth_session
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    for (family_id,) in &families {
        revoke_family(tx, *family_id, "membership_revoked").await?;
    }
    Ok(families.len() as u64)
}

pub async fn list_sessions(
    pool: &PgPool,
    user_id: Uuid,
    current_sid: Option<Uuid>,
) -> Result<Vec<SessionView>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        Uuid,
        Uuid,
        Option<String>,
        Option<String>,
        Option<String>,
        chrono::DateTime<Utc>,
        chrono::DateTime<Utc>,
    )> = sqlx::query_as(
        r#"
        SELECT id, org_id, device_label, user_agent, ip_address, created_at, last_seen_at
        FROM auth_session
        WHERE user_id = $1 AND revoked_at IS NULL
        ORDER BY last_seen_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| SessionView {
            id: r.0.to_string(),
            org_id: OrgId::new(r.1).to_public().as_str(),
            device_label: r.2,
            user_agent: r.3,
            ip_address: r.4,
            created_at: r.5.to_rfc3339(),
            last_seen_at: r.6.to_rfc3339(),
            current: current_sid == Some(r.0),
        })
        .collect())
}

pub fn refresh_cookie(token: &str, secure: bool) -> Cookie<'static> {
    let mut c = Cookie::build((REFRESH_COOKIE_NAME, token.to_string()))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/api/v1/auth")
        .max_age(time::Duration::seconds(
            companyos_auth_token::REFRESH_TOKEN_TTL_SECS,
        ))
        .build();
    if secure {
        c.set_secure(true);
    }
    c
}

pub fn clear_refresh_cookie(secure: bool) -> Cookie<'static> {
    let mut c = Cookie::build((REFRESH_COOKIE_NAME, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/api/v1/auth")
        .max_age(time::Duration::seconds(0))
        .build();
    if secure {
        c.set_secure(true);
    }
    c
}

#[allow(clippy::too_many_arguments)]
pub fn switch_org_access(
    ring: &KeyRing,
    user_id: Uuid,
    user_public: &str,
    org: OrgId,
    membership_id: Uuid,
    roles: &[String],
    policy_version: i64,
    session_id: Uuid,
    family_id: Uuid,
) -> Result<String, String> {
    let claims = AccessClaims {
        sub: user_public.to_string(),
        user_id,
        org_id: org.to_public().as_str(),
        org_uuid: org.as_uuid(),
        membership_id,
        roles: roles.to_vec(),
        policy_version,
        sid: session_id,
        family_id,
        jti: new_uuid_v7(),
        iss: "companyos".into(),
        iat: 0,
        exp: 0,
        api_key_id: None,
        scopes: None,
    };
    companyos_auth_token::mint_access_token(ring, claims, companyos_auth_token::access_ttl())
        .map_err(|e| e.to_string())
}

#[allow(dead_code)]
pub fn refresh_idle_window() -> Duration {
    Duration::days(30)
}
