use std::collections::HashMap;

use axum::extract::State;
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::set_session_org_id;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{PresignUploadRequest, PresignUploadResponse};
use crate::validate::validate_upload;

#[utoipa::path(
    post,
    path = "/api/v1/files/presign-upload",
    request_body = PresignUploadRequest,
    responses((status = 200, body = PresignUploadResponse)),
    tag = "files"
)]
pub async fn presign_upload(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<PresignUploadRequest>,
) -> Result<Json<PresignUploadResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    if !auth.local_bypass {
        let (principal, _, _) = load_principal(&state.pool, org_id, user_id, &request_id).await?;
        enforce(&principal, perms::platform_file_create(), &request_id)?;
    }

    validate_upload(&body.content_type, body.size_bytes)
        .map_err(|e| AppError::new(ErrorCode::ValidationFailed, &request_id, e))?;

    let id = new_uuid_v7();
    let public_id = format!("fil_{}", id.simple());
    let object_key = format!(
        "{}/{}/{}",
        org_id.as_uuid(),
        id,
        sanitize_filename(&body.filename)
    );

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
        INSERT INTO file_object
            (id, org_id, public_id, bucket, object_key, content_type, size_bytes, created_by, status)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'pending')
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(&public_id)
    .bind(&state.bucket)
    .bind(&object_key)
    .bind(&body.content_type)
    .bind(body.size_bytes)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), body.content_type.clone());

    let upload_url = if let Some(ep) = &state.minio_endpoint {
        // Prefer gateway/local-upload path that works without SigV4 clients.
        // When MinIO is set we still expose a signed-looking URL for path-style PUT,
        // and clients may also use PUT /api/v1/files/local-upload/{id} via gateway.
        match crate::presign::presign_put_object(
            ep,
            &state.bucket,
            &object_key,
            &state.minio_access_key,
            &state.minio_secret_key,
            &body.content_type,
            900,
        ) {
            Ok(url) => url,
            Err(_) => format!("http://127.0.0.1:8089/api/v1/files/local-upload/{public_id}"),
        }
    } else {
        format!("http://127.0.0.1:8089/api/v1/files/local-upload/{public_id}")
    };

    Ok(Json(PresignUploadResponse {
        upload_url,
        file_id: public_id,
        headers,
    }))
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
