//! Internal endpoint to flush deferred email digests.

use axum::extract::State;
use axum::Json;
use companyos_errors::AppError;
use serde::Serialize;
use utoipa::ToSchema;

use crate::digest::run_deferred_digest;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct DigestRunResponse {
    pub processed: u32,
}

#[utoipa::path(
    post,
    path = "/api/v1/notifications/internal/digest/run",
    responses((status = 200, body = DigestRunResponse)),
    tag = "notifications-internal"
)]
pub async fn run(State(state): State<AppState>) -> Result<Json<DigestRunResponse>, AppError> {
    let processed = run_deferred_digest(&state.pool).await?;
    Ok(Json(DigestRunResponse { processed }))
}
