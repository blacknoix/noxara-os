use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{ask, chat, documents, insights, proposals, sessions, settings, suggestions};
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
        version = "0.1.0",
        description = "Phase 1.9 — copilot, proposals, retrieval, document extract"
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
