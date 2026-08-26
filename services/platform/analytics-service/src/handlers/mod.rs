pub mod facts;
pub mod ingest;
pub mod reconcile;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/analytics/internal/ingest",
            post(ingest::ingest),
        )
        .route(
            "/api/v1/analytics/facts/invoice-issued",
            get(facts::invoice_issued),
        )
        .route(
            "/api/v1/analytics/reconcile/nightly",
            post(reconcile::nightly),
        )
}
