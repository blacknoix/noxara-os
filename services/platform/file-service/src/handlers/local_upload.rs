//! Local upload endpoint used when MinIO is unset (or as a fallback PUT target).
//! Stores bytes under `.tmp/files/{object_key}` and marks the file ready on next complete.

use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::auth::AuthCtx;
use crate::state::AppState;

fn storage_root() -> PathBuf {
    PathBuf::from(
        std::env::var("FILE_LOCAL_ROOT").unwrap_or_else(|_| ".tmp/files".into()),
    )
}

#[utoipa::path(
    put,
    path = "/api/v1/files/local-upload/{id}",
    params(("id" = String, Path, description = "file public id")),
    responses((status = 204)),
    tag = "files"
)]
pub async fn local_upload(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT object_key FROM file_object WHERE org_id = $1 AND public_id = $2",
    )
    .bind(org_id.as_uuid())
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some((object_key,)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "file not found",
        ));
    };

    let path = storage_root().join(&object_key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    }
    let mut file = fs::File::create(&path)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    file.write_all(&body)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/files/local-download/{id}",
    params(("id" = String, Path, description = "file public id")),
    responses((status = 200)),
    tag = "files"
)]
pub async fn local_download(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT object_key, content_type FROM file_object WHERE org_id = $1 AND public_id = $2",
    )
    .bind(org_id.as_uuid())
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some((object_key, content_type)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "file not found",
        ));
    };

    let path = storage_root().join(&object_key);
    let bytes = fs::read(&path)
        .await
        .map_err(|e| AppError::new(ErrorCode::NotFound, &request_id, e.to_string()))?;

    Ok((
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment".to_string(),
            ),
        ],
        bytes,
    ))
}
