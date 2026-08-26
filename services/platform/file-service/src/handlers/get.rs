use axum::extract::{Path, State};
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::FileMetaResponse;

#[utoipa::path(
    get,
    path = "/api/v1/files/{id}",
    params(("id" = String, Path, description = "file public id")),
    responses((status = 200, body = FileMetaResponse)),
    tag = "files"
)]
pub async fn get_file(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<FileMetaResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    if !auth.local_bypass {
        let (principal, _, _) = load_principal(&state.pool, org_id, user_id, &request_id).await?;
        enforce(&principal, perms::platform_file_read(), &request_id)?;
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let row: Option<(String, String, i64, String, String)> = sqlx::query_as(
        r#"
        SELECT public_id, content_type, size_bytes, status, object_key
        FROM file_object
        WHERE org_id = $1 AND public_id = $2
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some((file_id, content_type, size_bytes, status, object_key)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "file not found",
        ));
    };

    let download_url = if let Some(ep) = &state.minio_endpoint {
        format!(
            "{}/{}/{}?response-content-disposition=attachment",
            ep.trim_end_matches('/'),
            state.bucket,
            urlencoding::encode(&object_key)
        )
    } else {
        format!("http://127.0.0.1:8089/api/v1/files/local-download/{file_id}?disposition=attachment")
    };

    Ok(Json(FileMetaResponse {
        file_id,
        content_type,
        size_bytes,
        status,
        download_url,
    }))
}
