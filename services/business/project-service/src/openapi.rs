//! OpenAPI 3.1 document for the CompanyOS Operations API (`/api/v1/operations/...`).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

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
    ),
    components(schemas(
        ProjectDto,
        ProjectListResponse,
        CreateProjectRequest,
        UpdateProjectRequest,
        ChecklistItemDto,
        TaskAttachmentDto,
        TaskCommentDto,
        TaskDto,
        TaskListResponse,
        CreateTaskRequest,
        UpdateTaskRequest,
        MoveTaskRequest,
        CreateCommentRequest,
        CreateAttachmentRequest,
        BoardColumnDto,
        BoardResponse,
        CalendarEventDto,
        CalendarResponse,
        MyWorkResponse,
        SummaryResponse,
        ApplySalesEventRequest,
        ApplySalesEventResponse,
        NotificationIntentDto,
    )),
    tags(
        (name = "operations-projects", description = "Projects"),
        (name = "operations-tasks", description = "Tasks, move, attachments"),
        (name = "operations-comments", description = "Task comments and mentions"),
        (name = "operations-board", description = "Kanban board"),
        (name = "operations-my-work", description = "Assignee inbox"),
        (name = "operations-calendar", description = "Due-date calendar"),
        (name = "operations-summary", description = "Operations dashboard counts"),
        (name = "operations-events", description = "Sales event projection"),
    ),
    info(
        title = "CompanyOS Operations API",
        version = "0.1.0",
        description = "Phase 1.6 — Projects & Tasks (Operations bounded context)."
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
