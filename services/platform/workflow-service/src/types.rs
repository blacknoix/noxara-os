//! OpenAPI DTOs for `/api/v1/workflows/...`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::definition::WorkflowGraph;
use crate::simulate::{SimulateResult, SimulateStepResult};

pub use crate::catalogue::{ActionCatalogueEntry, TriggerCatalogueEntry};
pub use crate::simulate::{
    SimulateResult as SimulateResultDto, SimulateStepResult as SimulateStepResultDto,
};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowDefinitionDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub created_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_published_version: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Present on get when a draft graph exists / latest version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<WorkflowGraph>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowDefinitionListResponse {
    pub items: Vec<WorkflowDefinitionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateWorkflowDefinitionRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub graph: WorkflowGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateWorkflowDefinitionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<WorkflowGraph>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowVersionDto {
    pub id: String,
    pub definition_id: String,
    pub version: i32,
    pub graph: WorkflowGraph,
    pub required_permissions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowVersionListResponse {
    pub items: Vec<WorkflowVersionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PublishWorkflowRequest {
    /// Optional note; publish always creates a new immutable version from current draft graph.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StartWorkflowRequest {
    #[serde(default)]
    pub payload: serde_json::Value,
    /// When true, start is rejected — use /simulate instead (defense in depth).
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowInstanceDto {
    pub id: String,
    pub definition_id: String,
    pub version_id: String,
    pub version_number: i32,
    pub status: String,
    pub actor_user_id: String,
    pub temporal_workflow_id: String,
    pub step_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting_until: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sla_deadline: Option<DateTime<Utc>>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowInstanceListResponse {
    pub items: Vec<WorkflowInstanceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SimulateRequest {
    pub graph: WorkflowGraph,
    #[serde(default)]
    pub payload: serde_json::Value,
    /// Optional override; defaults to org max_steps_per_instance.
    #[serde(default)]
    pub max_steps: Option<i32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TriggerCatalogueResponse {
    pub items: Vec<TriggerCatalogueEntry>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ActionCatalogueResponse {
    pub items: Vec<ActionCatalogueEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FixtureWorkflowDto {
    pub name: String,
    pub description: String,
    pub graph: WorkflowGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FixtureListResponse {
    pub items: Vec<FixtureWorkflowDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OrgBoundsDto {
    pub max_concurrent: i32,
    pub max_steps_per_instance: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateOrgBoundsRequest {
    pub max_concurrent: i32,
    pub max_steps_per_instance: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MonitorSummaryDto {
    pub running: i64,
    pub waiting: i64,
    pub failed: i64,
    pub completed: i64,
    pub cancelled: i64,
    pub sla_breached: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MonitorResponse {
    pub summary: MonitorSummaryDto,
    pub instances: Vec<WorkflowInstanceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MigrateInstanceRequest {
    /// Stub: keep-old-version is the safe default; explicit migrate later.
    pub target_version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

// Re-export simulate types for OpenAPI components.
#[allow(dead_code)]
fn _schema_touch(_a: SimulateResult, _b: SimulateStepResult) {}
