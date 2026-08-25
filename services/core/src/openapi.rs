//! OpenAPI 3.1 document for the Hello resource (contract chain source).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::hello::{CreateHelloRequest, Hello, HelloListResponse};
use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::hello::list_hello,
        crate::hello::create_hello,
    ),
    components(schemas(Hello, CreateHelloRequest, HelloListResponse)),
    tags((name = "hello", description = "Phase 0 hello vertical slice")),
    info(
        title = "CompanyOS Core API",
        version = "0.1.0",
        description = "Phase 0 foundations — hello resource. LOCAL-ONLY auth."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

/// Write the OpenAPI document as pretty JSON (used by the codegen script).
#[allow(dead_code)]
pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
