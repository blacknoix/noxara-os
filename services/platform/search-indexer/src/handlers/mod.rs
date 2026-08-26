pub mod ingest;
pub mod query;
pub mod reindex;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/search/query", get(query::query))
        .route("/api/v1/search/internal/ingest", post(ingest::ingest))
        .route("/api/v1/search/reindex", post(reindex::reindex))
}
