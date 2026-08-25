//! SSO configuration data model + admin API surface.
//! Full IdP login is out of scope for Phase 1.1; endpoints return 403 when the
//! plan/feature flag is disabled.

use companyos_authz::{self as authz, perms, Principal, Role};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use super::sso_globally_enabled;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SsoConfigView {
    pub id: String,
    pub org_id: String,
    pub protocol: String,
    pub display_name: String,
    pub enabled: bool,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertSsoRequest {
    pub protocol: String,
    pub display_name: String,
    pub config: serde_json::Value,
    #[serde(default)]
    pub enabled: bool,
}

pub async fn sso_enabled_for_org(pool: &PgPool, org_id: Uuid) -> Result<bool, sqlx::Error> {
    if !sso_globally_enabled() {
        return Ok(false);
    }
    let row: Option<(bool,)> =
        sqlx::query_as("SELECT enabled FROM org_feature_flag WHERE org_id = $1 AND flag = 'sso'")
            .bind(org_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(e,)| e).unwrap_or(false))
}

pub fn ensure_sso_admin(roles: &[String], request_id: &str) -> Result<(), AppError> {
    let parsed: Vec<Role> = roles.iter().filter_map(|r| Role::parse(r)).collect();
    let principal = Principal::with_roles(parsed);
    if !authz::is_allowed(&principal, &perms::admin_sso_manage()) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "missing admin.sso.manage",
        ));
    }
    Ok(())
}

pub async fn require_sso_feature(
    pool: &PgPool,
    org_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    match sso_enabled_for_org(pool, org_id).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(AppError::new(
            ErrorCode::FeatureDisabled,
            request_id,
            "SSO is disabled for this organization (plan/feature flag)",
        )),
        Err(e) => Err(AppError::new(
            ErrorCode::Internal,
            request_id,
            e.to_string(),
        )),
    }
}

pub async fn list_configs(pool: &PgPool, org: OrgId) -> Result<Vec<SsoConfigView>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    set_session_org_id(&mut tx, org)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let rows: Vec<(Uuid, Uuid, String, String, String, bool, serde_json::Value)> = sqlx::query_as(
        r#"
        SELECT id, org_id, public_id, protocol, display_name, enabled, config
        FROM sso_configuration
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(rows
        .into_iter()
        .map(|r| SsoConfigView {
            id: r.2,
            org_id: OrgId::new(r.1).to_public().as_str(),
            protocol: r.3,
            display_name: r.4,
            enabled: r.5,
            config: r.6,
        })
        .collect())
}

pub async fn upsert_config(
    pool: &PgPool,
    org: OrgId,
    body: UpsertSsoRequest,
) -> Result<SsoConfigView, String> {
    if body.protocol != "saml" && body.protocol != "oidc" {
        return Err("protocol must be saml or oidc".into());
    }
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Org, id); // reuse org prefix? better custom — use hel-like
                                                    // Use a stable public id string without new IdKind for SSO: `sso_{uuid}`
    let public = format!("sso_{id}");
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    set_session_org_id(&mut tx, org)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        r#"
        INSERT INTO sso_configuration (id, org_id, public_id, protocol, display_name, config, enabled)
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(id)
    .bind(org.as_uuid())
    .bind(&public)
    .bind(&body.protocol)
    .bind(&body.display_name)
    .bind(&body.config)
    .bind(body.enabled)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    let _ = public_id;
    Ok(SsoConfigView {
        id: public,
        org_id: org.to_public().as_str(),
        protocol: body.protocol,
        display_name: body.display_name,
        enabled: body.enabled,
        config: body.config,
    })
}
