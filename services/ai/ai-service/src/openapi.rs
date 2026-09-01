use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{
    ask, chat, documents, insights, meeting_summaries, proposals, sessions, settings, suggestions,
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
    )),
    tags(
        (name = "ai", description = "CompanyOS AI copilot"),
    ),
    info(
        title = "CompanyOS AI API",
        version = "0.3.5",
        description = "Phase 3.5 — insights agents, meeting summaries, multi-context Q&A"
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
