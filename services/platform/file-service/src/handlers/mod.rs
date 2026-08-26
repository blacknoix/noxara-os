pub mod complete;
pub mod get;
pub mod local_upload;
pub mod presign;

use axum::routing::{get, post, put};
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/files/presign-upload",
            post(presign::presign_upload),
        )
        .route(
            "/api/v1/files/local-upload/{id}",
            put(local_upload::local_upload),
        )
        .route(
            "/api/v1/files/local-download/{id}",
            get(local_upload::local_download),
        )
        .route("/api/v1/files/{id}/complete", post(complete::complete))
        .route("/api/v1/files/{id}", get(get::get_file))
}
