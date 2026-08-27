//! OpenAPI 3.1 document for the CompanyOS People / HR API (`/api/v1/people/...`).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{
    assets, compensation, contracts, documents, employees, me, offboarding, onboarding, timeline,
};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        employees::list_employees,
        employees::create_employee,
        employees::get_employee,
        employees::update_employee,
        me::get_me,
        me::patch_me,
        compensation::list_compensation,
        compensation::create_compensation,
        contracts::list_contracts,
        contracts::create_contract,
        documents::list_documents,
        documents::create_document,
        assets::list_assets,
        assets::create_asset,
        timeline::get_timeline,
        onboarding::onboard,
        offboarding::offboard,
        offboarding::access_audit,
    ),
    components(schemas(
        EmployeeDto,
        EmployeeListResponse,
        CreateEmployeeRequest,
        UpdateEmployeeRequest,
        UpdateSelfProfileRequest,
        CompensationComponentDto,
        CompensationListResponse,
        CreateCompensationRequest,
        ContractDto,
        ContractListResponse,
        CreateContractRequest,
        DocumentDto,
        DocumentListResponse,
        CreateDocumentRequest,
        AssetDto,
        AssetListResponse,
        CreateAssetRequest,
        TaskDto,
        TaskListResponse,
        TimelineEventDto,
        TimelineResponse,
        OnboardRequest,
        OnboardResponse,
        OffboardRequest,
        OffboardResponse,
        AccessChecklistItem,
        AccessAuditResponse,
    )),
    tags(
        (name = "people-employees", description = "Employee directory and profile"),
        (name = "people-me", description = "Self-service non-restricted profile"),
        (name = "people-compensation", description = "Encrypted compensation components"),
        (name = "people-contracts", description = "Employment contracts"),
        (name = "people-documents", description = "HR documents (opaque fil_ ids)"),
        (name = "people-assets", description = "HR asset assignments"),
        (name = "people-timeline", description = "Employee activity timeline"),
        (name = "people-onboarding", description = "EmployeeOnboarding saga"),
        (name = "people-offboarding", description = "EmployeeOffboarding + access audit"),
    ),
    info(
        title = "CompanyOS People / HR API",
        version = "0.1.0",
        description = "Phase 2.1 People (HR) — employees, compensation, onboarding/offboarding."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/people/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
