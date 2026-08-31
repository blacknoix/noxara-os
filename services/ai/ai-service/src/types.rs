//! DTOs for AI API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Citation {
    pub record_type: String,
    pub record_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolTraceEntry {
    pub tool_name: String,
    pub permission: String,
    pub decision: String,
    pub reason: String,
    pub args_summary: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChatRequest {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TokenUsage {
    pub model: String,
    pub prompt_template_version: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u32,
    pub cost_estimate_minor: i64,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProposalView {
    pub id: String,
    pub tool_name: String,
    pub action_type: String,
    pub status: String,
    pub command: serde_json::Value,
    pub rendered_diff: String,
    pub citations: Vec<Citation>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChatResponse {
    pub session_id: String,
    pub interaction_id: String,
    pub role: String,
    pub content: String,
    pub citations: Vec<Citation>,
    pub follow_ups: Vec<String>,
    pub proposals: Vec<ProposalView>,
    pub tool_trace: Vec<ToolTraceEntry>,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConfirmProposalRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AskRequest {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AskFormField {
    pub name: String,
    pub label: String,
    pub value: String,
    #[serde(rename = "type")]
    pub field_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AskForm {
    pub action_type: String,
    pub fields: Vec<AskFormField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AskResponse {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<AskForm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<Citation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_trace: Option<Vec<ToolTraceEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataSharingSettings {
    pub share_with_provider: bool,
    pub allow_training: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModulesEnabled {
    pub copilot: bool,
    pub insights: bool,
    pub document_ai: bool,
    pub ask_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AiSettings {
    pub modules_enabled: ModulesEnabled,
    pub model_preference: String,
    pub auto_execute_allow_list: Vec<String>,
    pub data_sharing: DataSharingSettings,
    pub monthly_token_budget: i64,
    pub tokens_used_this_month: i64,
    pub budget_month: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateAiSettingsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules_enabled: Option<ModulesEnabled>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_preference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_execute_allow_list: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_sharing: Option<DataSharingSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_token_budget: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InsightObservation {
    pub id: String,
    pub title: String,
    pub body: String,
    pub evidence: Vec<Citation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
    pub estimate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insight_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action_detail: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InsightsResponse {
    pub observations: Vec<InsightObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InsightsRefreshResponse {
    pub created: u32,
    pub observations: Vec<InsightObservation>,
    pub pending_proposals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateMeetingSummaryRequest {
    pub calendar_event_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MeetingSummaryView {
    pub id: String,
    pub public_id: String,
    pub calendar_event_id: String,
    pub calendar_connector: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    pub summary_markdown: String,
    pub action_items: serde_json::Value,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MeetingSummariesListResponse {
    pub items: Vec<MeetingSummaryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DocumentExtractRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DocumentReview {
    pub id: String,
    pub kind: String,
    pub confidence: f64,
    pub extracted: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_scope: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SuggestionChip {
    pub id: String,
    pub label: String,
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionDetail {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_scope: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub interactions: Vec<ChatResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProposalsListResponse {
    pub items: Vec<ProposalView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionsListResponse {
    pub items: Vec<SessionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SuggestionsResponse {
    pub chips: Vec<SuggestionChip>,
}
