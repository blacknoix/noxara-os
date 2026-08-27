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

// ---------------------------------------------------------------------------
// Phase 2.2 — Attendance & Leave
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkScheduleDto {
    pub id: String,
    pub name: String,
    pub timezone: String,
    pub weekly_hours: serde_json::Value,
    pub location: Option<String>,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkScheduleListResponse {
    pub items: Vec<WorkScheduleDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateWorkScheduleRequest {
    pub name: String,
    pub timezone: Option<String>,
    pub weekly_hours: Option<serde_json::Value>,
    pub location: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HolidayDto {
    pub id: String,
    pub name: String,
    pub holiday_date: String,
    pub location: Option<String>,
    pub is_half_day: bool,
    pub half_day_period: Option<String>,
    pub created_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HolidayListResponse {
    pub items: Vec<HolidayDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateHolidayRequest {
    pub name: String,
    pub holiday_date: String,
    pub location: Option<String>,
    pub is_half_day: Option<bool>,
    pub half_day_period: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams, Default)]
pub struct AttendanceListQuery {
    pub employee_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttendanceDto {
    pub id: String,
    pub employee_id: String,
    pub entry_kind: String,
    pub recorded_at: String,
    pub local_date: String,
    pub timezone: String,
    pub source: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub accuracy_meters: Option<f64>,
    pub note: Option<String>,
    pub reverses_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttendanceListResponse {
    pub items: Vec<AttendanceDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecordAttendanceRequest {
    pub employee_id: Option<String>,
    pub entry_kind: String,
    pub recorded_at: Option<String>,
    pub timezone: Option<String>,
    pub source: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub accuracy_meters: Option<f64>,
    pub note: Option<String>,
    /// Public id of the fact row to reverse (creates append-only reversal).
    pub reverses_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttendanceImportRequest {
    /// CSV body: employee_id,entry_kind,recorded_at[,timezone,latitude,longitude,accuracy_meters,note]
    pub csv: String,
    pub batch_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttendanceImportResponse {
    pub imported: i64,
    pub skipped: i64,
    pub items: Vec<AttendanceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveTypeDto {
    pub id: String,
    pub code: String,
    pub name: String,
    pub category: String,
    pub accrual_cadence: String,
    pub accrual_units_milli: i32,
    pub carry_forward_cap_milli: Option<i32>,
    pub expiry_days: Option<i32>,
    pub allows_half_day: bool,
    pub requires_approval: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveTypeListResponse {
    pub items: Vec<LeaveTypeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateLeaveTypeRequest {
    pub code: String,
    pub name: String,
    pub category: Option<String>,
    pub accrual_cadence: Option<String>,
    pub accrual_units_milli: Option<i32>,
    pub carry_forward_cap_milli: Option<i32>,
    pub expiry_days: Option<i32>,
    pub allows_half_day: Option<bool>,
    pub requires_approval: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveRequestDto {
    pub id: String,
    pub employee_id: String,
    pub leave_type_id: String,
    pub status: String,
    pub start_date: String,
    pub end_date: String,
    pub start_period: String,
    pub end_period: String,
    pub units_milli: i32,
    pub units_days: String,
    pub timezone: String,
    pub reason: Option<String>,
    pub approval_id: Option<String>,
    pub decided_at: Option<String>,
    pub decision_note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveRequestListResponse {
    pub items: Vec<LeaveRequestDto>,
    pub total: i64,
}

#[derive(Debug, Deserialize, IntoParams, Default)]
pub struct LeaveRequestListQuery {
    pub employee_id: Option<String>,
    pub status: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateLeaveRequestRequest {
    pub employee_id: Option<String>,
    pub leave_type_id: String,
    pub start_date: String,
    pub end_date: String,
    pub start_period: Option<String>,
    pub end_period: Option<String>,
    pub timezone: Option<String>,
    pub reason: Option<String>,
    /// When true, immediately submit into the approval engine.
    pub submit: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DecideLeaveRequest {
    pub approve: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveBalanceDto {
    pub employee_id: String,
    pub leave_type_id: String,
    pub leave_type_code: String,
    pub leave_type_name: String,
    pub balance_units_milli: i32,
    pub balance_days: String,
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveBalanceListResponse {
    pub items: Vec<LeaveBalanceDto>,
}

#[derive(Debug, Deserialize, IntoParams, Default)]
pub struct LeaveBalanceQuery {
    pub employee_id: Option<String>,
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveCalendarEntryDto {
    pub leave_request_id: String,
    pub employee_id: String,
    pub employee_display_name: String,
    pub leave_type_code: String,
    pub status: String,
    pub start_date: String,
    pub end_date: String,
    pub start_period: String,
    pub end_period: String,
    pub units_milli: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveCalendarResponse {
    pub items: Vec<LeaveCalendarEntryDto>,
}

#[derive(Debug, Deserialize, IntoParams, Default)]
pub struct LeaveCalendarQuery {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AbsenceReportRowDto {
    pub employee_id: String,
    pub employee_display_name: String,
    pub leave_type_code: String,
    pub units_milli: i32,
    pub units_days: String,
    pub request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AbsenceReportResponse {
    pub items: Vec<AbsenceReportRowDto>,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, IntoParams, Default)]
pub struct AbsenceReportQuery {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CarryForwardRequest {
    pub year: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CarryForwardResponse {
    pub workflow_id: String,
    pub year: i32,
    pub status: String,
    pub entries_posted: i32,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AccrueLeaveRequest {
    pub employee_id: String,
    pub leave_type_id: String,
    pub units_milli: Option<i32>,
    pub effective_date: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaveLedgerEntryDto {
    pub id: String,
    pub employee_id: String,
    pub leave_type_id: String,
    pub entry_kind: String,
    pub units_milli: i32,
    pub effective_date: String,
    pub expires_on: Option<String>,
    pub leave_request_id: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Phase 2.3 — Payroll
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PayrollComponentDto {
    pub id: String,
    pub code: String,
    pub label: String,
    pub line_kind: String,
    pub calc_method: String,
    #[schema(value_type = Object)]
    pub config_json: serde_json::Value,
    pub currency: Option<String>,
    pub is_active: bool,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PayrollComponentListResponse {
    pub items: Vec<PayrollComponentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreatePayrollComponentRequest {
    pub code: String,
    pub label: String,
    pub line_kind: String,
    pub calc_method: String,
    #[schema(value_type = Object)]
    pub config_json: Option<serde_json::Value>,
    pub currency: Option<String>,
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PayrollRunDto {
    pub id: String,
    pub status: String,
    pub period_start: String,
    pub period_end: String,
    pub currency: String,
    pub adjustment_of_run_id: Option<String>,
    pub approval_id: Option<String>,
    pub journal_public_id: Option<String>,
    pub employee_count: i32,
    pub gross_minor: i64,
    pub deductions_minor: i64,
    pub net_minor: i64,
    pub calculated_at: Option<String>,
    pub approved_at: Option<String>,
    pub paid_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PayrollRunListResponse {
    pub items: Vec<PayrollRunDto>,
    pub total: i64,
}

#[derive(Debug, Deserialize, IntoParams, Default)]
pub struct PayrollRunListQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreatePayrollRunRequest {
    pub period_start: String,
    pub period_end: String,
    pub currency: String,
    pub adjustment_of_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PayslipLineDto {
    pub id: String,
    pub line_kind: String,
    pub component_code: String,
    pub label: String,
    pub amount_minor: i64,
    pub currency: String,
    #[schema(value_type = Object)]
    pub calculation_basis: serde_json::Value,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PayslipDto {
    pub id: String,
    pub run_id: String,
    pub employee_id: String,
    pub currency: String,
    pub gross_minor: i64,
    pub deductions_minor: i64,
    pub net_minor: i64,
    pub status: String,
    pub issued_at: Option<String>,
    pub lines: Vec<PayslipLineDto>,
    pub created_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PayslipListResponse {
    pub items: Vec<PayslipDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DecidePayrollRequest {
    pub approve: bool,
    pub note: Option<String>,
}
