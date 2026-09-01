use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::agents::action::AiActionView;
use crate::agents::kill_switch::{KillSwitchView, SetKillSwitchRequest};
use crate::agents::nl_workflow::{NlWorkflowDraft, NlWorkflowRequest};
use crate::agents::policy::AgentPolicyDoc;
use crate::agents::prompt_pack::{PromptPackDoc, PromptPackView};
use crate::agents::review::{AgentReviewReport, AgentTypeStats};
use crate::agents::runtime::{AgentRunOutcome, AgentRunView, StartRunRequest};
use crate::handlers::{
    agents, ask, chat, documents, insights, meeting_summaries, proposals, sessions, settings,
    suggestions,
};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        chat::chat,
        chat::chat_stream,
        sessions::list_sessions,
        sessions::get_session,
        settings::get_settings,
        settings::patch_settings,
        insights::get_insights,
        insights::refresh_insights,
        meeting_summaries::list_meeting_summaries,
        meeting_summaries::get_meeting_summary,
        meeting_summaries::create_from_calendar,
        meeting_summaries::accept_meeting_summary,
        meeting_summaries::reject_meeting_summary,
        proposals::list_proposals,
        proposals::confirm_proposal,
        proposals::cancel_proposal,
        ask::ask,
        documents::extract_document,
        documents::get_document_review,
        suggestions::get_suggestions,
        agents::get_policy,
        agents::list_policies,
        agents::publish_policy,
        agents::get_kill,
        agents::set_kill,
        agents::list_runs,
        agents::start_run,
        agents::get_run,
        agents::reverse_action,
        agents::get_prompt_pack,
        agents::put_prompt_pack,
        agents::propose_workflow,
        agents::get_review,
        agents::seed_review_fixture,
    ),
    components(schemas(
        Citation,
        ToolTraceEntry,
        ChatRequest,
        ChatResponse,
        TokenUsage,
        ProposalView,
        ConfirmProposalRequest,
        AskRequest,
        AskResponse,
        AskForm,
        AskFormField,
        AiSettings,
        ModulesEnabled,
        DataSharingSettings,
        UpdateAiSettingsRequest,
        InsightObservation,
        InsightsResponse,
        InsightsRefreshResponse,
        CreateMeetingSummaryRequest,
        MeetingSummaryView,
        MeetingSummariesListResponse,
        DocumentExtractRequest,
        DocumentReview,
        SessionSummary,
        SessionDetail,
        SessionsListResponse,
        ProposalsListResponse,
        MessageResponse,
        SuggestionChip,
        SuggestionsResponse,
        AgentPolicyDoc,
        agents::PolicyView,
        agents::PolicyListResponse,
        KillSwitchView,
        SetKillSwitchRequest,
        StartRunRequest,
        AgentRunView,
        AgentRunOutcome,
        agents::RunsListResponse,
        AiActionView,
        PromptPackDoc,
        PromptPackView,
        NlWorkflowRequest,
        NlWorkflowDraft,
        AgentReviewReport,
        AgentTypeStats,
    )),
    tags(
        (name = "ai", description = "CompanyOS AI copilot"),
        (name = "ai-agents", description = "Phase 4.3 governed autonomous agents"),
    ),
    info(
        title = "CompanyOS AI API",
        version = "0.4.3",
        description = "Phase 4.3 — governed agents, kill switch, NL workflow drafts, review pack"
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/ai/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
