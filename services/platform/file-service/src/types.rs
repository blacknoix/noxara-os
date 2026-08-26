use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PresignUploadRequest {
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PresignUploadResponse {
    pub upload_url: String,
    pub file_id: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FileMetaResponse {
    pub file_id: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub status: String,
    /// Download URL. Clients SHOULD set `Content-Disposition: attachment`
    /// when proxying to force download rather than inline render.
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}
