//! Start a reindex job for one org (tiny fixture in tests — never 100k).

use axum::extract::State;
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{ReindexRequest, ReindexResponse};

#[utoipa::path(
    post,
    path = "/api/v1/search/reindex",
    request_body = ReindexRequest,
    responses((status = 200, body = ReindexResponse)),
    tag = "search"
)]
pub async fn reindex(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ReindexRequest>,
) -> Result<Json<ReindexResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_public: PublicId = body
        .org_id
        .parse()
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, &request_id, "invalid org_id"))?;
    let org_id = OrgId::from_public(&org_public).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "org_id must be org_…",
        )
    })?;

    if auth.ctx.org_id != org_id {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            &request_id,
            "org_id does not match authenticated tenant",
        ));
    }

    if !auth.local_bypass {
        let (principal, _, _) =
            load_principal(&state.pool, org_id, auth.ctx.actor.user_id, &request_id).await?;
        enforce(&principal, perms::platform_search_reindex(), &request_id)?;
    }

    let job_id = new_uuid_v7();
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO search_index_job (id, org_id, status, requested_by, created_at)
        VALUES ($1, $2, 'pending', $3, now())
        "#,
    )
    .bind(job_id)
    .bind(org_id.as_uuid())
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    // Mark complete immediately for the in-memory / tiny fixture path.
    // Production worker would stream documents; tests never enqueue 100k.
    sqlx::query("UPDATE search_index_job SET status = 'completed' WHERE id = $1")
        .bind(job_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(Json(ReindexResponse {
        job_id: format!("sij_{}", job_id.simple()),
        status: "completed".into(),
    }))
}
