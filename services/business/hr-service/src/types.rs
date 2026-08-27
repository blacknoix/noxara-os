//! Request/response DTOs for `/api/v1/people/...`.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

pub const EMPLOYEE_STATUSES: &[&str] = &[
    "draft",
    "onboarding",
    "active",
    "on_leave",
    "offboarding",
    "terminated",
];

#[derive(Debug, Deserialize, IntoParams, Default)]
pub struct ListQuery {
    pub status: Option<String>,
    pub department_id: Option<String>,
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Directory / non-sensitive employee projection.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EmployeeDto {
    pub id: String,
    pub user_id: Option<String>,
    pub display_name: String,
    pub legal_first_name: Option<String>,
    pub legal_last_name: Option<String>,
    pub work_email: Option<String>,
    pub personal_email: Option<String>,
    pub phone: Option<String>,
    pub title: Option<String>,
    pub status: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub location: Option<String>,
    pub department_id: Option<String>,
    pub manager_employee_id: Option<String>,
    pub owner_user_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
    /// Present only when caller holds `hr.employee.read_sensitive`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub government_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EmployeeListResponse {
    pub items: Vec<EmployeeDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateEmployeeRequest {
    pub display_name: String,
    pub user_id: Option<String>,
    pub legal_first_name: Option<String>,
    pub legal_last_name: Option<String>,
    pub work_email: Option<String>,
    pub personal_email: Option<String>,
    pub phone: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub start_date: Option<String>,
    pub location: Option<String>,
    /// Opaque Workspace department public id (`dep_…`).
    pub department_id: Option<String>,
    /// Manager employee public id (`emp_…`).
    pub manager_employee_id: Option<String>,
    /// Restricted — encrypted at rest; requires write + stored for sensitive read.
    pub government_id: Option<String>,
    pub bank_details: Option<String>,
    pub tax_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateEmployeeRequest {
    pub display_name: Option<String>,
    pub user_id: Option<String>,
    pub legal_first_name: Option<String>,
    pub legal_last_name: Option<String>,
    pub work_email: Option<String>,
    pub personal_email: Option<String>,
    pub phone: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub location: Option<String>,
    pub department_id: Option<String>,
    pub manager_employee_id: Option<String>,
    pub government_id: Option<String>,
    pub bank_details: Option<String>,
    pub tax_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateSelfProfileRequest {
    pub display_name: Option<String>,
    pub personal_email: Option<String>,
    pub phone: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompensationComponentDto {
    pub id: String,
    pub employee_id: String,
    pub contract_id: Option<String>,
    pub component_type: String,
    pub label: String,
    pub amount_minor: i64,
    pub currency: String,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub created_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompensationListResponse {
    pub items: Vec<CompensationComponentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateCompensationRequest {
    pub component_type: Option<String>,
    pub label: String,
    pub amount_minor: i64,
    pub currency: String,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub contract_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ContractDto {
    pub id: String,
    pub employee_id: String,
    pub contract_type: String,
    pub title: Option<String>,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ContractListResponse {
    pub items: Vec<ContractDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateContractRequest {
    pub contract_type: Option<String>,
    pub title: Option<String>,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DocumentDto {
    pub id: String,
    pub employee_id: String,
    pub title: String,
    pub doc_type: String,
    pub file_id: Option<String>,
    pub expires_at: Option<String>,
    pub collected: bool,
    pub created_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DocumentListResponse {
    pub items: Vec<DocumentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDocumentRequest {
    pub title: String,
    pub doc_type: Option<String>,
    pub file_id: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AssetDto {
    pub id: String,
    pub employee_id: String,
    pub label: String,
    pub asset_tag: Option<String>,
    pub status: String,
    pub assigned_at: String,
    pub returned_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AssetListResponse {
    pub items: Vec<AssetDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateAssetRequest {
    pub label: String,
    pub asset_tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HrTaskDto {
    pub id: String,
    pub employee_id: String,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub assignee_user_id: Option<String>,
    pub due_at: Option<String>,
    pub completed_at: Option<String>,
    pub workflow_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HrTaskListResponse {
    pub items: Vec<HrTaskDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TimelineEventDto {
    pub id: String,
    pub event_type: String,
    pub summary: String,
    pub metadata: serde_json::Value,
    pub occurred_at: String,
    pub actor_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TimelineResponse {
    pub items: Vec<TimelineEventDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OnboardRequest {
    pub display_name: String,
    pub work_email: Option<String>,
    pub title: Option<String>,
    pub start_date: Option<String>,
    pub department_id: Option<String>,
    pub manager_employee_id: Option<String>,
    /// Link existing user (`usr_…`); otherwise user_id stays null until linked.
    pub user_id: Option<String>,
    /// Role key to assign via membership update activity (opaque; applied by access step).
    pub role: Option<String>,
    pub asset_labels: Option<Vec<String>>,
    pub document_titles: Option<Vec<String>>,
    pub task_titles: Option<Vec<String>>,
    /// Test hook: inject activity failure after this step for compensation tests.
    pub fail_after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OnboardResponse {
    pub employee: EmployeeDto,
    pub workflow_id: String,
    pub tasks: Vec<HrTaskDto>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OffboardRequest {
    pub end_date: Option<String>,
    pub reassign_manager_to: Option<String>,
    pub reason: Option<String>,
    pub fail_after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AccessChecklistItem {
    pub path: String,
    pub cleared: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OffboardResponse {
    pub employee: EmployeeDto,
    pub workflow_id: String,
    pub checklist: Vec<AccessChecklistItem>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AccessAuditResponse {
    pub employee_id: String,
    pub user_id: Option<String>,
    pub checklist: Vec<AccessChecklistItem>,
    pub all_cleared: bool,
}
