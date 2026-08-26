//! OpenAPI 3.1 document for the CompanyOS Operations API (`/api/v1/operations/...`).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::approvals::handlers as approvals;
use crate::approvals::types::*;
use crate::handlers::{board, calendar, comments, events, my_work, projects, summary, tasks};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        projects::list_projects,
        projects::create_project,
        projects::get_project,
        projects::update_project,
        projects::delete_project,
        tasks::list_tasks,
        tasks::create_task,
        tasks::get_task,
        tasks::update_task,
        tasks::delete_task,
        tasks::move_task,
        tasks::create_attachment,
        comments::list_comments,
        comments::create_comment,
        board::get_board,
        my_work::my_work,
        calendar::get_calendar,
        summary::get_summary,
        events::apply_sales_event,
        approvals::list_approvals,
        approvals::create_approval,
        approvals::get_approval,
        approvals::decide_approval,
        approvals::bulk_decide,
        approvals::inbox_summary,
        approvals::escalate_approval,
        approvals::create_delegation,
        approvals::list_delegations,
        approvals::list_policies,
        approvals::create_policy,
        approvals::get_policy,
        approvals::update_policy,
    ),
    components(schemas(
        ProjectDto,
        ProjectListResponse,
        CreateProjectRequest,
        UpdateProjectRequest,
        TaskDto,
        TaskListResponse,
        CreateTaskRequest,
        UpdateTaskRequest,
        MoveTaskRequest,
        ChecklistItemDto,
        TaskAttachmentDto,
        TaskCommentDto,
        CreateCommentRequest,
        CreateAttachmentRequest,
        BoardColumnDto,
        TaskBoardResponse,
        CalendarEventDto,
        CalendarResponse,
        MyWorkResponse,
        SummaryResponse,
        ApplySalesEventRequest,
        ApplySalesEventResponse,
        NotificationIntentDto,
        ApprovalMode,
        PolicyMatch,
        PolicyStepDef,
        PolicyDefinition,
        ResolvedStepSnapshot,
        RoutingSnapshot,
        ApprovalStepDto,
        ApprovalDto,
        ApprovalListResponse,
        CreateApprovalRequest,
        DecideApprovalRequest,
        BulkDecideRequest,
        BulkDecideResponse,
        CreateDelegationRequest,
        DelegationDto,
        ApprovalPolicyDto,
        CreatePolicyRequest,
        UpdatePolicyRequest,
        PolicyListResponse,
        InboxSummaryDto,
    )),
    tags(
        (name = "operations-projects", description = "Projects (Operations)"),
        (name = "operations-tasks", description = "Tasks, board moves, attachments"),
        (name = "operations-comments", description = "Task comments and @mentions"),
        (name = "operations-board", description = "Kanban board"),
        (name = "operations-my-work", description = "Cross-project My Work"),
        (name = "operations-calendar", description = "Due-date calendar"),
        (name = "operations-summary", description = "Dashboard aggregates"),
        (name = "operations-events", description = "In-process sales event apply"),
        (name = "operations-approvals", description = "Approval engine (Phase 1.7)"),
    ),
    info(
        title = "CompanyOS Operations API",
        version = "0.1.0",
        description = "Phase 1.6 Projects & Tasks + Phase 1.7 Approval engine."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/operations/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
