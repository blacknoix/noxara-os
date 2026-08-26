pub mod digest_run;
pub mod feed;
pub mod ingest;
pub mod preferences;
pub mod sse;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/notifications/feed", get(feed::feed))
        .route("/api/v1/notifications/{id}/read", post(feed::mark_read))
        .route(
            "/api/v1/notifications/preferences",
            get(preferences::get_preferences).put(preferences::put_preferences),
        )
        .route(
            "/api/v1/notifications/internal/ingest",
            post(ingest::ingest),
        )
        .route(
            "/api/v1/notifications/internal/digest/run",
            post(digest_run::run),
        )
        .route("/api/v1/notifications/sse-token", get(sse::sse_token))
}
