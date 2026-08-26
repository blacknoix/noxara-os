use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{facts, ingest, reconcile};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(ingest::ingest, facts::invoice_issued, reconcile::nightly),
    components(schemas(
        InvoiceIssuedFact,
        FactsResponse,
        IngestResponse,
        ReconcileResponse,
    )),
    tags(
        (name = "analytics", description = "Event-derived analytics facts (ADR-011)"),
        (name = "analytics-internal", description = "Event ingest"),
    ),
    info(
        title = "CompanyOS Analytics API",
        version = "0.1.0",
        description = "Phase 1.8 — facts from events only (ADR-011)."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/analytics/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
