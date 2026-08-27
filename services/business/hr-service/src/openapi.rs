//! OpenAPI 3.1 document for the CompanyOS People / HR API (`/api/v1/people/...`).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{
    assets, attendance, compensation, contracts, documents, employees, leave, me, offboarding,
    onboarding, schedules, timeline,
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
        schedules::list_schedules,
        schedules::create_schedule,
        schedules::get_schedule,
        schedules::list_holidays,
        schedules::create_holiday,
        schedules::get_holiday,
        attendance::list_attendance,
        attendance::list_my_attendance,
        attendance::record_attendance,
        attendance::import_attendance,
        leave::list_leave_types,
        leave::create_leave_type,
        leave::get_leave_type,
        leave::list_leave_requests,
        leave::list_my_leave,
        leave::create_leave_request,
        leave::get_leave_request,
        leave::submit_leave_request,
        leave::cancel_leave_request,
        leave::decide_leave_request,
        leave::list_balances,
        leave::team_calendar,
        leave::absence_report,
        leave::run_carry_forward,
        leave::accrue_leave,
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
        HrTaskDto,
        HrTaskListResponse,
        TimelineEventDto,
        TimelineResponse,
        OnboardRequest,
        OnboardResponse,
        OffboardRequest,
        OffboardResponse,
        AccessChecklistItem,
        AccessAuditResponse,
        WorkScheduleDto,
        WorkScheduleListResponse,
        CreateWorkScheduleRequest,
        HolidayDto,
        HolidayListResponse,
        CreateHolidayRequest,
        AttendanceDto,
        AttendanceListResponse,
        RecordAttendanceRequest,
        AttendanceImportRequest,
        AttendanceImportResponse,
        LeaveTypeDto,
        LeaveTypeListResponse,
        CreateLeaveTypeRequest,
        LeaveRequestDto,
        LeaveRequestListResponse,
        CreateLeaveRequestRequest,
        DecideLeaveRequest,
        LeaveBalanceDto,
        LeaveBalanceListResponse,
        LeaveCalendarEntryDto,
        LeaveCalendarResponse,
        AbsenceReportRowDto,
        AbsenceReportResponse,
        CarryForwardRequest,
        CarryForwardResponse,
        AccrueLeaveRequest,
        LeaveLedgerEntryDto,
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
        (name = "people-schedules", description = "Work schedules"),
        (name = "people-holidays", description = "Holiday calendars"),
        (name = "people-attendance", description = "Append-only attendance"),
        (name = "people-leave", description = "Leave types, requests, balances, carry-forward"),
    ),
    info(
        title = "CompanyOS People / HR API",
        version = "0.2.0",
        description = "Phase 2.1–2.2 People (HR) — employees, attendance, leave."
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
