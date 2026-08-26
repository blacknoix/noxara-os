//! SSE token for gateway proxy (stub).

use axum::Json;
use companyos_errors::AppError;
use companyos_ids::new_uuid_v7;

use crate::auth::AuthCtx;
use crate::types::SseTokenResponse;

#[utoipa::path(
    get,
    path = "/api/v1/notifications/sse-token",
    responses((status = 200, body = SseTokenResponse)),
    tag = "notifications"
)]
pub async fn sse_token(auth: AuthCtx) -> Result<Json<SseTokenResponse>, AppError> {
    let _ = auth;
    // Gateway SSE proxy will validate; this stub returns an opaque session token.
    Ok(Json(SseTokenResponse {
        token: format!("sse_{}", new_uuid_v7().simple()),
        expires_in_secs: 3600,
    }))
}
