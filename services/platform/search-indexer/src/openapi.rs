use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{ingest, query, reindex};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(query::query, ingest::ingest, reindex::reindex),
    components(schemas(
        SearchHit,
        QueryResponse,
        IngestResponse,
        ReindexRequest,
        ReindexResponse,
        MessageResponse,
    )),
    tags(
        (name = "search", description = "Tenant-scoped search"),
        (name = "search-internal", description = "Event ingest"),
    ),
    info(
        title = "CompanyOS Search API",
        version = "0.1.0",
        description = "Phase 1.8 — search indexer with authz re-check per hit."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/search/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
