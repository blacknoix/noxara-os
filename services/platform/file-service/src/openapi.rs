use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{complete, get, presign};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(presign::presign_upload, complete::complete, get::get_file),
    components(schemas(
        PresignUploadRequest,
        PresignUploadResponse,
        FileMetaResponse,
        MessageResponse,
    )),
    tags((name = "files", description = "Presigned upload and file metadata")),
    info(
        title = "CompanyOS File API",
        version = "0.1.0",
        description = "Phase 1.8 — file uploads (MinIO or local stub). Download URLs use Content-Disposition: attachment guidance."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/files/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
