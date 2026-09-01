//! Customer-managed encryption keys (CMEK) — wrap key for per-org DEKs.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_crypto::{generate_wrapped_dek, rotate_org_key, CmkId, Kms, OrgDataKey, WrappedDek};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::extract::AuthUser;
use crate::governance::{authorize, internal};
use crate::state::AppState;
use crate::workspace;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/governance/cmk", get(get_cmk).post(provision_cmk))
        .route("/api/v1/governance/cmk/rotate", post(rotate_cmk))
        .route("/api/v1/governance/cmk/revoke", post(revoke_cmk))
}

#[derive(Debug, Serialize)]
pub struct CmkStatusDto {
    pub configured: bool,
    pub public_id: Option<String>,
    pub provider_key_id: Option<String>,
    pub status: Option<String>,
    pub dek_version: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct ProvisionCmkRequest {
    #[serde(default = "default_alias")]
    pub alias: String,
}

fn default_alias() -> String {
    "org-cmk".into()
}

#[derive(Debug, Serialize)]
pub struct CmkMutationResponse {
    pub public_id: String,
    pub provider_key_id: String,
    pub status: String,
    pub dek_version: i32,
}

/// Load org wrapped DEK for encrypt/decrypt helpers (tests + internal).
pub async fn load_wrapped_dek(
    state: &AppState,
    org_id: Uuid,
    request_id: &str,
) -> Result<WrappedDek, AppError> {
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, companyos_tenancy::OrgId::new(org_id))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let row: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT c.provider_key_id, d.wrapped_dek_b64
        FROM org_data_key d
        JOIN org_cmk c ON c.id = d.cmk_id
        WHERE d.org_id = $1 AND c.status = 'active'
        "#,
    )
    .bind(org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    let Some((provider_key_id, wrapped_b64)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "org CMEK not configured",
        ));
    };
    Ok(WrappedDek {
        version: companyos_crypto::WRAPPED_DEK_VERSION,
        cmk_id: CmkId(provider_key_id),
        wrapped_b64,
    })
}

pub async fn org_data_key(
    state: &AppState,
    org_id: Uuid,
    request_id: &str,
) -> Result<OrgDataKey, AppError> {
    let wrapped = load_wrapped_dek(state, org_id, request_id).await?;
    OrgDataKey::from_wrapped(state.kms.as_ref(), &wrapped).map_err(|e| {
        AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!("decrypt failed: {e}"),
        )
    })
}

async fn get_cmk(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<CmkStatusDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_cmk_read()).await?;
    let org_id = user.ctx.org_id.as_uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<(String, String, String, i32)> = sqlx::query_as(
        r#"
        SELECT c.public_id, c.provider_key_id, c.status, d.version
        FROM org_cmk c
        LEFT JOIN org_data_key d ON d.cmk_id = c.id AND d.org_id = c.org_id
        WHERE c.org_id = $1 AND c.status IN ('active', 'rotating')
        ORDER BY c.created_at DESC
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(match row {
        Some((public_id, provider_key_id, status, dek_version)) => CmkStatusDto {
            configured: true,
            public_id: Some(public_id),
            provider_key_id: Some(provider_key_id),
            status: Some(status),
            dek_version: Some(dek_version),
        },
        None => CmkStatusDto {
            configured: false,
            public_id: None,
            provider_key_id: None,
            status: None,
            dek_version: None,
        },
    }))
}

async fn provision_cmk(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ProvisionCmkRequest>,
) -> Result<Json<CmkMutationResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_cmk_rotate()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let existing: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_cmk WHERE org_id = $1 AND status = 'active'")
            .bind(org_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    if existing.is_some() {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            "CMEK already provisioned; use rotate",
        ));
    }

    let cmk = state
        .kms
        .create_key(&format!("{}-{}", body.alias, org_id))
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let wrapped = generate_wrapped_dek(state.kms.as_ref(), &cmk)
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let cmk_uuid = new_uuid_v7();
    let public_id = PublicId::new(IdKind::CustomerManagedKey, cmk_uuid);
    sqlx::query(
        r#"
        INSERT INTO org_cmk (id, org_id, public_id, provider_key_id, status, created_by)
        VALUES ($1,$2,$3,$4,'active',$5)
        "#,
    )
    .bind(cmk_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(cmk.as_str())
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dek_id = new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO org_data_key (id, org_id, cmk_id, wrapped_dek_b64, version)
        VALUES ($1,$2,$3,$4,1)
        "#,
    )
    .bind(dek_id)
    .bind(org_id)
    .bind(cmk_uuid)
    .bind(&wrapped.wrapped_b64)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "cmk.provision",
        "org_cmk",
        &public_id.to_string(),
        serde_json::json!({ "provider_key_id": cmk.as_str() }),
    )
    .await;

    Ok(Json(CmkMutationResponse {
        public_id: public_id.to_string(),
        provider_key_id: cmk.0,
        status: "active".into(),
        dek_version: 1,
    }))
}

async fn rotate_cmk(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<CmkMutationResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_cmk_rotate()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    let old_wrapped = load_wrapped_dek(&state, org_id, &request_id).await?;
    let rot = rotate_org_key(
        state.kms.as_ref(),
        &old_wrapped,
        &format!("org-{org_id}-rotated"),
        false,
    )
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    // Mark previous active keys rotating (still usable until revoke).
    sqlx::query("UPDATE org_cmk SET status = 'rotating' WHERE org_id = $1 AND status = 'active'")
        .bind(org_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let cmk_uuid = new_uuid_v7();
    let public_id = PublicId::new(IdKind::CustomerManagedKey, cmk_uuid);
    sqlx::query(
        r#"
        INSERT INTO org_cmk (id, org_id, public_id, provider_key_id, status, created_by)
        VALUES ($1,$2,$3,$4,'active',$5)
        "#,
    )
    .bind(cmk_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(rot.new_cmk_id.as_str())
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let version: i32 = sqlx::query_scalar(
        "UPDATE org_data_key SET cmk_id = $2, wrapped_dek_b64 = $3, version = version + 1
         WHERE org_id = $1 RETURNING version",
    )
    .bind(org_id)
    .bind(cmk_uuid)
    .bind(&rot.wrapped_dek.wrapped_b64)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "cmk.rotate",
        "org_cmk",
        &public_id.to_string(),
        serde_json::json!({
            "new_provider_key_id": rot.new_cmk_id.as_str(),
            "old_provider_key_id": rot.old_cmk_id.as_str(),
            "dek_version": version,
        }),
    )
    .await;

    Ok(Json(CmkMutationResponse {
        public_id: public_id.to_string(),
        provider_key_id: rot.new_cmk_id.0,
        status: "active".into(),
        dek_version: version,
    }))
}

#[derive(Debug, Deserialize)]
pub struct RevokeCmkRequest {
    /// Provider key id to revoke (defaults to previous rotating key, else active).
    pub provider_key_id: Option<String>,
}

async fn revoke_cmk(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<RevokeCmkRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_cmk_revoke()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let target: Option<(Uuid, String)> = if let Some(ref pid) = body.provider_key_id {
        sqlx::query_as(
            "SELECT id, provider_key_id FROM org_cmk WHERE org_id = $1 AND provider_key_id = $2",
        )
        .bind(org_id)
        .bind(pid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?
    } else {
        // Prefer rotating (old) key; fall back to active only if explicitly alone.
        let rotating: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT id, provider_key_id FROM org_cmk WHERE org_id = $1 AND status = 'rotating'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(org_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if rotating.is_some() {
            rotating
        } else {
            sqlx::query_as(
                "SELECT id, provider_key_id FROM org_cmk WHERE org_id = $1 AND status = 'active'
                 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(org_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?
        }
    };

    let Some((cmk_uuid, provider_key_id)) = target else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "CMK not found",
        ));
    };

    state
        .kms
        .revoke(&CmkId(provider_key_id.clone()))
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        "UPDATE org_cmk SET status = 'revoked', revoked_at = now() WHERE id = $1 AND org_id = $2",
    )
    .bind(cmk_uuid)
    .bind(org_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "cmk.revoke",
        "org_cmk",
        &provider_key_id,
        serde_json::json!({ "provider_key_id": provider_key_id }),
    )
    .await;

    Ok(Json(serde_json::json!({
        "revoked": true,
        "provider_key_id": provider_key_id,
    })))
}
