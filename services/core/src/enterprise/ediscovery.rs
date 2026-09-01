//! Legal hold + durable eDiscovery / audit export jobs.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::extract::AuthUser;
use crate::governance::audit_verify;
use crate::governance::{authorize, internal};
use crate::state::AppState;
use crate::workspace;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/governance/ediscovery/holds",
            get(list_holds).post(create_hold),
        )
        .route(
            "/api/v1/governance/ediscovery/exports",
            get(list_exports).post(create_export),
        )
        .route(
            "/api/v1/governance/ediscovery/exports/{id}",
            get(get_export),
        )
        .route(
            "/api/v1/governance/ediscovery/exports/{id}/download",
            get(download_export),
        )
}

#[derive(Debug, Deserialize)]
pub struct CreateHoldRequest {
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct LegalHoldDto {
    pub id: String,
    pub reason: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateExportRequest {
    #[serde(default = "default_contexts")]
    pub include_contexts: Vec<String>,
    pub legal_hold_id: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_contexts() -> Vec<String> {
    vec!["audit".into()]
}
fn default_kind() -> String {
    "ediscovery".into()
}

#[derive(Debug, Serialize)]
pub struct ExportJobDto {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub include_contexts: Vec<String>,
    pub hash_chain_ok: Option<bool>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

async fn create_hold(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateHoldRequest>,
) -> Result<Json<LegalHoldDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_ediscovery_export()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    if body.reason.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "reason required",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::LegalHold, id);
    let created_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO legal_hold (id, org_id, public_id, reason, active, created_by)
        VALUES ($1,$2,$3,$4,true,$5) RETURNING created_at
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.reason.trim())
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "ediscovery.hold.create",
        "legal_hold",
        &public_id.to_string(),
        serde_json::json!({ "reason": body.reason }),
    )
    .await;

    Ok(Json(LegalHoldDto {
        id: public_id.to_string(),
        reason: body.reason,
        active: true,
        created_at,
    }))
}

async fn list_holds(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<LegalHoldDto>>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_ediscovery_export()).await?;
    let org_id = user.ctx.org_id.as_uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<(String, String, bool, DateTime<Utc>)> = sqlx::query_as(
        "SELECT public_id, reason, active, created_at FROM legal_hold WHERE org_id = $1 ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, reason, active, created_at)| LegalHoldDto {
                id,
                reason,
                active,
                created_at,
            })
            .collect(),
    ))
}

async fn create_export(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateExportRequest>,
) -> Result<Json<ExportJobDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_ediscovery_export()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    let kind = body.kind.clone();
    if !matches!(kind.as_str(), "ediscovery" | "audit" | "consolidation") {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "kind must be ediscovery|audit|consolidation",
        ));
    }

    let legal_hold_uuid = if let Some(ref hid) = body.legal_hold_id {
        let pid: PublicId = hid.parse().map_err(|_| {
            AppError::new(
                ErrorCode::ValidationFailed,
                &request_id,
                "invalid legal_hold_id",
            )
        })?;
        Some(pid.uuid())
    } else {
        None
    };

    // Verify hash chain across all partitions before packaging.
    let verify = audit_verify::verify_all(&state.pool, user.ctx.org_id, &request_id).await?;
    let hash_chain_ok = verify.ok;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let audit_rows: Vec<(
        Uuid,
        Uuid,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        r#"
        SELECT id, actor_user_id, action, resource_type, resource_id,
               prev_hash, content_hash, created_at
        FROM audit_entry WHERE org_id = $1
        ORDER BY created_at, id
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let pack = serde_json::json!({
        "org_id": PublicId::new(IdKind::Org, org_id).to_string(),
        "kind": kind,
        "exported_at": Utc::now(),
        "hash_chain_ok": hash_chain_ok,
        "verify": verify,
        "include_contexts": body.include_contexts,
        "audit_entries": audit_rows.iter().map(|(id, actor, action, rt, rid, prev, content, at)| {
            serde_json::json!({
                "id": id.to_string(),
                "actor_user_id": actor.to_string(),
                "action": action,
                "resource_type": rt,
                "resource_id": rid,
                "prev_hash": prev,
                "content_hash": content,
                "created_at": at,
            })
        }).collect::<Vec<_>>(),
    });
    let file_bytes = serde_json::to_vec_pretty(&pack)
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::ExportJob, id);
    let file_public_id = format!("file_export_{}", public_id.as_str());
    let expires_at = Utc::now() + Duration::days(30);
    let completed_at = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO export_job (
            id, org_id, public_id, kind, status, include_contexts, legal_hold_id,
            file_public_id, file_bytes, content_type, hash_chain_ok,
            created_by, completed_at, expires_at
        ) VALUES (
            $1,$2,$3,$4,'completed',$5,$6,$7,$8,'application/json',$9,$10,$11,$12
        )
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&kind)
    .bind(&body.include_contexts)
    .bind(legal_hold_uuid)
    .bind(&file_public_id)
    .bind(&file_bytes)
    .bind(hash_chain_ok)
    .bind(actor)
    .bind(completed_at)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "ediscovery.export.create",
        "export_job",
        &public_id.to_string(),
        serde_json::json!({
            "kind": kind,
            "hash_chain_ok": hash_chain_ok,
            "bytes": file_bytes.len(),
        }),
    )
    .await;

    Ok(Json(ExportJobDto {
        id: public_id.to_string(),
        kind,
        status: "completed".into(),
        include_contexts: body.include_contexts,
        hash_chain_ok: Some(hash_chain_ok),
        created_at: completed_at,
        completed_at: Some(completed_at),
        expires_at: Some(expires_at),
        error_message: None,
    }))
}

async fn list_exports(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ExportJobDto>>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_ediscovery_export()).await?;
    let org_id = user.ctx.org_id.as_uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<(
        String,
        String,
        String,
        Vec<String>,
        Option<bool>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT public_id, kind, status, include_contexts, hash_chain_ok,
               created_at, completed_at, expires_at, error_message
        FROM export_job WHERE org_id = $1 ORDER BY created_at DESC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    kind,
                    status,
                    include_contexts,
                    hash_chain_ok,
                    created_at,
                    completed_at,
                    expires_at,
                    error_message,
                )| ExportJobDto {
                    id,
                    kind,
                    status,
                    include_contexts,
                    hash_chain_ok,
                    created_at,
                    completed_at,
                    expires_at,
                    error_message,
                },
            )
            .collect(),
    ))
}

async fn get_export(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ExportJobDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_ediscovery_export()).await?;
    let list = list_exports(State(state), user).await?.0;
    list.into_iter()
        .find(|e| e.id == id)
        .map(Json)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, request_id, "export not found"))
}

async fn download_export(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_ediscovery_export()).await?;
    let org_id = user.ctx.org_id.as_uuid();

    let pid: PublicId = id
        .parse()
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, &request_id, "invalid id"))?;
    if pid.kind() != IdKind::ExportJob {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "id must be exj_…",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<(Vec<u8>, String)> = sqlx::query_as(
        "SELECT file_bytes, COALESCE(content_type, 'application/json')
         FROM export_job WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(pid.uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    let Some((bytes, content_type)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "export not found",
        ));
    };

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{id}.json\""),
            ),
        ],
        bytes,
    )
        .into_response())
}
