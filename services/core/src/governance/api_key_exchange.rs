//! Resolve an organization API key into a short-lived access JWT.
//!
//! Lookup is by `key_hash` via `app.api_key_lookup` RLS policy (before org_id
//! is known). Effective scopes = key scopes ∩ owner role permissions.

use chrono::{Duration, Utc};
use companyos_auth_token::{mint_access_token, AccessClaims};
use companyos_authz::{is_allowed, PermissionId, Principal};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_outbox::insert_event;
use companyos_tenancy::{set_api_key_lookup, set_session_org_id, Actor, OrgId};
use sqlx::PgPool;
use uuid::Uuid;

use super::types::ApiKeyExchangeResponse;
use super::{internal, tenancy_internal};
use crate::public_scopes;
use crate::state::AppState;
use crate::workspace::principal::load_principal;

type ApiKeyLookupRow = (
    Uuid,
    Uuid,
    String,
    serde_json::Value,
    Option<chrono::DateTime<Utc>>,
    Option<chrono::DateTime<Utc>>,
    Uuid,
    i32,
);

/// Exchange a key hash for an access token + metadata for the gateway.
pub async fn exchange(
    state: &AppState,
    key_hash: &str,
    request_id: &str,
) -> Result<ApiKeyExchangeResponse, AppError> {
    let row: Option<ApiKeyLookupRow> = {
        let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
        set_api_key_lookup(&mut tx)
            .await
            .map_err(tenancy_internal(request_id))?;
        let row = sqlx::query_as(
            r#"
            SELECT id, org_id, public_id, scopes, expires_at, revoked_at,
                   created_by, rate_limit_per_minute
            FROM org_api_key
            WHERE key_hash = $1
            "#,
        )
        .bind(key_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(request_id))?;
        tx.commit().await.map_err(internal(request_id))?;
        row
    };

    let Some((
        key_uuid,
        org_uuid,
        public_id,
        scopes_json,
        expires_at,
        revoked_at,
        created_by,
        rate_limit_per_minute,
    )) = row
    else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid API key",
        ));
    };

    if revoked_at.is_some() {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "API key revoked",
        ));
    }
    if expires_at.is_some_and(|e| e <= Utc::now()) {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "API key expired",
        ));
    }

    let org_id = OrgId::new(org_uuid);
    let requested: Vec<String> = serde_json::from_value(scopes_json).unwrap_or_default();

    let (owner_principal, policy_version, membership_id) =
        load_principal(&state.pool, org_id, created_by, request_id).await?;

    let effective = intersect_scopes(&owner_principal, &requested);
    if effective.is_empty() {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "API key scopes do not intersect owner permissions",
        ));
    }

    {
        let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
        set_session_org_id(&mut tx, org_id)
            .await
            .map_err(tenancy_internal(request_id))?;
        sqlx::query(
            "UPDATE org_api_key SET last_used_at = now(), updated_at = now() WHERE id = $1 AND org_id = $2",
        )
        .bind(key_uuid)
        .bind(org_uuid)
        .execute(&mut *tx)
        .await
        .map_err(internal(request_id))?;
        tx.commit().await.map_err(internal(request_id))?;
    }

    let org_public = org_id.to_public().as_str();
    let user_public = PublicId::new(IdKind::User, created_by).as_str();
    let now = Utc::now();
    let region = {
        let mut conn = state.pool.acquire().await.map_err(internal(request_id))?;
        crate::auth::sessions::load_org_region(&mut conn, org_id)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e))?
    };
    let claims = AccessClaims {
        sub: user_public,
        user_id: created_by,
        org_id: org_public.clone(),
        org_uuid,
        membership_id,
        roles: vec![],
        policy_version,
        sid: key_uuid,
        family_id: key_uuid,
        jti: Uuid::now_v7(),
        iss: "companyos".into(),
        iat: now.timestamp(),
        exp: (now + Duration::minutes(5)).timestamp(),
        api_key_id: Some(public_id.clone()),
        scopes: Some(effective.clone()),
        region,
    };

    let access_token = mint_access_token(&state.auth_keys.ring, claims, Duration::minutes(5))
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(ApiKeyExchangeResponse {
        access_token,
        api_key_id: public_id,
        org_id: org_public,
        scopes: effective,
        rate_limit_per_minute,
        rate_limit_rpm: Some(rate_limit_per_minute),
    })
}

fn intersect_scopes(owner: &Principal, requested: &[String]) -> Vec<String> {
    requested
        .iter()
        .filter(|s| public_scopes::is_public_scope(s))
        .filter(|s| is_allowed(owner, &PermissionId::from(s.as_str())))
        .cloned()
        .collect()
}

/// Record a usage row for analytics (called by gateway via internal endpoint).
pub async fn record_usage(
    pool: &PgPool,
    org_id: OrgId,
    api_key_public_id: &str,
    route: &str,
    method: &str,
    status_code: i32,
    duration_ms: i32,
    request_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;

    let key_id: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_api_key WHERE org_id = $1 AND public_id = $2")
            .bind(org_id.as_uuid())
            .bind(api_key_public_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(request_id))?;
    let Some((key_id,)) = key_id else {
        return Ok(());
    };

    let id = new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO api_key_usage (id, org_id, api_key_id, route, method, status_code, duration_ms)
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(key_id)
    .bind(route)
    .bind(method)
    .bind(status_code)
    .bind(duration_ms)
    .execute(&mut *tx)
    .await
    .map_err(internal(request_id))?;

    let envelope = EventEnvelope::new(
        org_id,
        Context::Admin,
        "api_request",
        "recorded",
        1,
        Actor::human(Uuid::nil()),
        serde_json::json!({
            "api_key_id": api_key_public_id,
            "route": route,
            "method": method,
            "status_code": status_code,
            "duration_ms": duration_ms,
        }),
    );
    insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit().await.map_err(internal(request_id))?;
    Ok(())
}
