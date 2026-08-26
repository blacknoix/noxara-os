pub mod complete;
pub mod get;
pub mod presign;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/files/presign-upload", post(presign::presign_upload))
        .route("/api/v1/files/{id}/complete", post(complete::complete))
        .route("/api/v1/files/{id}", get(get::get_file))
}
