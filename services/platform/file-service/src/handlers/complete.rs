use axum::extract::{Path, State};
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::MessageResponse;

#[utoipa::path(
    post,
    path = "/api/v1/files/{id}/complete",
    params(("id" = String, Path, description = "file public id")),
    responses((status = 200, body = MessageResponse)),
    tag = "files"
)]
pub async fn complete(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    if !auth.local_bypass {
        let (principal, _, _) = load_principal(&state.pool, org_id, user_id, &request_id).await?;
        enforce(&principal, perms::platform_file_create(), &request_id)?;
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let result =
        sqlx::query("UPDATE file_object SET status = 'ready' WHERE org_id = $1 AND public_id = $2")
            .bind(org_id.as_uuid())
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "file not found",
        ));
    }
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(Json(MessageResponse {
        message: "upload complete".into(),
    }))
}
