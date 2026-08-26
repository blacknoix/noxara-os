use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(health_doc),
    tags((name = "workflow-host", description = "Temporal workflow catalogue host")),
    info(
        title = "CompanyOS Workflow Host",
        version = "0.1.0",
        description = "Phase 1.8 — platform Temporal workflow catalogue (ApprovalProcess, InvoiceDunning, DataImport, TenantDeletion, …)."
    )
)]
pub struct ApiDoc;

#[utoipa::path(
    get,
    path = "/healthz",
    responses((status = 200, description = "host health")),
    tag = "workflow-host"
)]
#[allow(dead_code)]
fn health_doc() {}

pub fn router() -> Router {
    Router::new().route(
        "/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
