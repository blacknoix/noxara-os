//! Request/response DTOs for `/api/v1/operations/...`.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

pub const TASK_STATUSES: &[&str] = &["backlog", "todo", "in_progress", "in_review", "done"];
pub const TASK_PRIORITIES: &[&str] = &["low", "medium", "high", "urgent"];
pub const PROJECT_STATUSES: &[&str] = &["active", "on_hold", "completed", "cancelled"];

#[derive(Debug, Deserialize, IntoParams, Default)]
pub struct ListQuery {
    pub status: Option<String>,
    pub project_id: Option<String>,
    pub assignee_id: Option<String>,
    pub q: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub owner_user_id: String,
    pub customer_id: Option<String>,
    pub deal_id: Option<String>,
    pub starts_at: Option<String>,
    pub due_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectListResponse {
    pub items: Vec<ProjectDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
    pub status: Option<String>,
    /// Opaque Sales customer public id (`cus_…`) — stored as UUID, never joined.
    pub customer_id: Option<String>,
    /// Opaque Sales deal public id (`dl_…`).
    pub deal_id: Option<String>,
    pub starts_at: Option<String>,
    pub due_at: Option<String>,
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub customer_id: Option<String>,
    pub deal_id: Option<String>,
    pub starts_at: Option<String>,
    pub due_at: Option<String>,
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChecklistItemDto {
    pub id: String,
    pub title: String,
    pub is_done: bool,
    pub position: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskAttachmentDto {
    pub id: String,
    pub file_name: String,
    pub content_type: Option<String>,
    pub byte_size: Option<i64>,
    pub url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskCommentDto {
    pub id: String,
    pub author_user_id: String,
    pub body: String,
    pub created_at: String,
    pub mentioned_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub owner_user_id: String,
    pub assignee_id: Option<String>,
    pub due_at: Option<String>,
    pub completed_at: Option<String>,
    pub labels: Vec<String>,
    pub position: f64,
    pub blocked_by: Vec<String>,
    pub checklist: Vec<ChecklistItemDto>,
    pub attachments: Vec<TaskAttachmentDto>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskListResponse {
    pub items: Vec<TaskDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTaskRequest {
    pub project_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub assignee_id: Option<String>,
    pub due_at: Option<String>,
    pub labels: Option<Vec<String>>,
    pub checklist: Option<Vec<String>>,
    pub blocked_by: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub assignee_id: Option<String>,
    pub due_at: Option<String>,
    pub labels: Option<Vec<String>>,
    pub position: Option<f64>,
    pub blocked_by: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MoveTaskRequest {
    pub status: String,
    pub position: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateCommentRequest {
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateAttachmentRequest {
    pub file_name: String,
    pub url: String,
    pub content_type: Option<String>,
    pub byte_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BoardColumnDto {
    pub status: String,
    pub tasks: Vec<TaskDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskBoardResponse {
    pub project_id: Option<String>,
    pub columns: Vec<BoardColumnDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CalendarEventDto {
    pub id: String,
    pub title: String,
    pub due_at: String,
    pub status: String,
    pub project_id: String,
    pub assignee_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CalendarResponse {
    pub events: Vec<CalendarEventDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MyWorkResponse {
    pub assigned: Vec<TaskDto>,
    pub mentions: Vec<TaskCommentDto>,
    pub total_assigned: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SummaryResponse {
    pub open_tasks: i64,
    pub my_open_tasks: i64,
    pub projects_active: i64,
    pub overdue: i64,
    /// Pending approvals assigned to the current user (Phase 1.7).
    #[serde(default)]
    pub pending_approvals_for_me: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApplySalesEventRequest {
    pub envelope: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApplySalesEventResponse {
    pub applied: bool,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NotificationIntentDto {
    pub id: String,
    pub recipient_user_id: String,
    pub resource_type: String,
    pub resource_id: String,
    pub kind: String,
    pub body_preview: Option<String>,
    pub created_at: String,
}
