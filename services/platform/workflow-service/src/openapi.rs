//! OpenAPI 3.1 document for `/api/v1/workflows/...`.

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::definition::{BranchArm, HumanStepKind, WorkflowGraph, WorkflowNode, WorkflowTrigger};
use crate::handlers::{catalogue, definitions, instances, monitor, simulate};
use crate::simulate::{SimulateResult, SimulateStepResult};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        catalogue::list_triggers,
        catalogue::list_actions,
        catalogue::list_fixtures,
        definitions::list_definitions,
        definitions::create_definition,
        definitions::get_definition,
        definitions::update_definition,
        definitions::publish_definition,
        definitions::list_versions,
        definitions::get_version,
        definitions::migrate_stub,
        instances::start_instance,
        instances::list_instances,
        instances::get_instance,
        instances::cancel_instance,
        simulate::simulate_handler,
        monitor::monitor,
        monitor::get_bounds,
        monitor::update_bounds,
    ),
    components(schemas(
        WorkflowGraph,
        WorkflowTrigger,
        WorkflowNode,
        BranchArm,
        HumanStepKind,
        WorkflowDefinitionDto,
        WorkflowDefinitionListResponse,
        CreateWorkflowDefinitionRequest,
        UpdateWorkflowDefinitionRequest,
        WorkflowVersionDto,
        WorkflowVersionListResponse,
        PublishWorkflowRequest,
        StartWorkflowRequest,
        WorkflowInstanceDto,
        WorkflowInstanceListResponse,
        SimulateRequest,
        SimulateResult,
        SimulateStepResult,
        TriggerCatalogueEntry,
        ActionCatalogueEntry,
        TriggerCatalogueResponse,
        ActionCatalogueResponse,
        FixtureWorkflowDto,
        FixtureListResponse,
        OrgBoundsDto,
        UpdateOrgBoundsRequest,
        MonitorSummaryDto,
        MonitorResponse,
        MigrateInstanceRequest,
        MessageResponse,
    )),
    tags(
        (name = "workflows-catalogue", description = "Trigger/action catalogues and fixtures"),
        (name = "workflows-definitions", description = "Definition CRUD + versioned publish"),
        (name = "workflows-instances", description = "Start, list, cancel instances"),
        (name = "workflows-simulate", description = "Dry-run simulation (zero side effects)"),
        (name = "workflows-monitor", description = "Monitor running/waiting/failed/SLA + bounds"),
    ),
    info(
        title = "CompanyOS Workflow API",
        version = "0.1.0",
        description = "Phase 3.1 Configurable workflow engine — org-scoped definitions, Temporal UserWorkflow execution, permission-checked actions, dry-run simulation."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/workflows/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
