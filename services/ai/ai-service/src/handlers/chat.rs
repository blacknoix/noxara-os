//! POST /api/v1/ai/chat and streaming variant.

use axum::extract::State;
use axum::http::{HeaderMap, Method};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Json, Router};
use axum::routing::post;
use companyos_authz::perms;
use companyos_ids::new_uuid_v7;
use companyos_tenancy::set_session_org_id;
use futures::stream::Stream;
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::handlers::common::{
    build_usage, check_token_budget, extract_bearer, load_settings, record_token_usage,
    resolve_principal, enforce_perm,
};
use crate::provider::{CompletionRequest, wrap_untrusted};
use crate::retrieval::{hybrid_retrieve, RetrievalQuery};
use crate::state::AppState;
use crate::tools::{persist_proposal, run_tool, ToolOutcome};
use crate::types::{ChatRequest, ChatResponse};
use companyos_errors::{AppError, ErrorCode};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ai/chat", post(chat))
        .route("/api/v1/ai/chat/stream", post(chat_stream))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/chat",
    request_body = ChatRequest,
    responses((status = 200, body = ChatResponse)),
    tag = "ai"
)]
pub async fn chat(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
    let bearer = extract_bearer(&headers).unwrap_or_default();
    let response = process_chat(&state, &auth, &bearer, &req).await?;
    Ok(Json(response))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/chat/stream",
    request_body = ChatRequest,
    responses((status = 200, description = "SSE stream")),
    tag = "ai"
)]
pub async fn chat_stream(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let bearer = extract_bearer(&headers).unwrap_or_default();
    let response = process_chat(&state, &auth, &bearer, &req).await?;

    let stream = async_stream::stream! {
        for token in response.content.split_whitespace() {
            yield Ok(Event::default().event("token").data(format!("{token} ")));
        }
        for citation in &response.citations {
            let data = serde_json::to_string(citation).unwrap_or_default();
            yield Ok(Event::default().event("citation").data(data));
        }
        for proposal in &response.proposals {
            let data = serde_json::to_string(proposal).unwrap_or_default();
            yield Ok(Event::default().event("proposal").data(data));
        }
        let done = json!({
            "session_id": response.session_id,
            "interaction_id": response.interaction_id,
        });
        yield Ok(Event::default().event("done").data(done.to_string()));
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

async fn process_chat(
    state: &AppState,
    auth: &AuthCtx,
    bearer: &str,
    req: &ChatRequest,
) -> Result<ChatResponse, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(state, auth).await?;
    enforce_perm(&principal, perms::ai_copilot_use(), &request_id)?;

    let settings = load_settings(state, org_id, &request_id).await?;
    if !settings.modules_enabled.copilot {
        return Err(AppError::new(
            ErrorCode::FeatureDisabled,
            &request_id,
            "copilot module disabled",
        ));
    }
    check_token_budget(&settings)?;

    let session_id = resolve_session(state, auth, req).await?;
    let interaction_id = new_uuid_v7();

    let org_public = org_id.to_public().as_str();
    let retrieval = RetrievalQuery::new(Some(&org_public), &req.message)?;
    let citations = if bearer.is_empty() {
        Vec::new()
    } else {
        hybrid_retrieve(state, auth, retrieval, bearer).await?
    };

    let page_context = req
        .page_scope
        .as_deref()
        .map(wrap_untrusted)
        .unwrap_or_default();

    let completion_req = CompletionRequest {
        system: format!(
            "CompanyOS copilot v{}. Cite sources. Writes require user confirmation.",
            state.prompt_template_version
        ),
        user_message: req.message.clone(),
        context: page_context,
        citations: citations.clone(),
    };

    let completion = state
        .provider
        .complete(completion_req)
        .await
        .map_err(|e| AppError::new(ErrorCode::ServiceUnavailable, &request_id, e))?;

    let mut tool_trace = Vec::new();
    let mut proposals = Vec::new();

    for tool_name in &completion.suggested_tools {
        let args = default_tool_args(tool_name, req);
        let outcome = run_tool(
            state,
            &principal,
            tool_name,
            &args,
            bearer,
            org_id,
            user_id,
            &request_id,
        )
        .await;
        match outcome {
            ToolOutcome::Read(_, trace) => tool_trace.push(trace),
            ToolOutcome::Proposal(draft, trace) => {
                tool_trace.push(trace);
                let proposal_id = persist_proposal(
                    state,
                    org_id.as_uuid(),
                    user_id,
                    Some(interaction_id),
                    &draft,
                    &request_id,
                )
                .await?;
                let view = crate::handlers::common::load_proposal_view(
                    state,
                    org_id,
                    proposal_id,
                    &request_id,
                )
                .await?;
                proposals.push(view);
            }
            ToolOutcome::Denied(trace) => tool_trace.push(trace),
        }
    }

  // Auto-execute only when explicitly in allow_list
    if !settings.auto_execute_allow_list.is_empty() {
        for proposal in &proposals {
            if settings.auto_execute_allow_list.contains(&proposal.action_type) {
                // auto-execute skipped by default policy — confirm required unless in list
            }
        }
    }

    let usage = build_usage(state, &completion);
    let total_tokens = usage.input_tokens + usage.output_tokens;
    record_token_usage(state, org_id, total_tokens, &request_id).await?;

    persist_interaction(
        state,
        org_id.as_uuid(),
        session_id,
        user_id,
        interaction_id,
        &completion.content,
        &citations,
        &tool_trace,
        &usage,
        &request_id,
    )
    .await?;

    let follow_ups = vec![
        "Show overdue invoices".into(),
        "Summarize open deals".into(),
    ];

    Ok(ChatResponse {
        session_id: session_id.to_string(),
        interaction_id: interaction_id.to_string(),
        role: "assistant".into(),
        content: completion.content,
        citations,
        follow_ups,
        proposals,
        tool_trace,
        usage,
    })
}

fn default_tool_args(tool_name: &str, req: &ChatRequest) -> serde_json::Value {
    match tool_name {
        "create_invoice" => json!({
            "customer_id": "cus_placeholder",
            "amount_minor": 10000,
            "currency": "USD",
        }),
        "create_task" => json!({
            "project_id": "prj_placeholder",
            "title": req.message.chars().take(60).collect::<String>(),
        }),
        "create_expense" => json!({
            "amount_minor": 5000,
            "currency": "USD",
            "description": req.message.chars().take(80).collect::<String>(),
        }),
        "draft_follow_up_activity" => json!({
            "subject": "Follow up",
            "body": req.message.chars().take(200).collect::<String>(),
            "deal_id": parse_scope_record(req.page_scope.as_deref(), "deal"),
        }),
        "create_deal_note" => json!({
            "body": req.message.chars().take(200).collect::<String>(),
            "deal_id": parse_scope_record(req.page_scope.as_deref(), "deal"),
        }),
        _ => json!({}),
    }
}

fn parse_scope_record(scope: Option<&str>, prefix: &str) -> Option<String> {
    scope.and_then(|s| {
        if s.starts_with(prefix) {
            s.split(':').nth(1).map(String::from)
        } else {
            None
        }
    })
}

async fn resolve_session(
    state: &AppState,
    auth: &AuthCtx,
    req: &ChatRequest,
) -> Result<Uuid, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    if let Some(sid) = req.session_id.as_deref() {
        if let Ok(uuid) = Uuid::parse_str(sid) {
            return Ok(uuid);
        }
    }

    let session_id = new_uuid_v7();
    let title = req.message.chars().take(48).collect::<String>();

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_session (id, org_id, user_id, title, page_scope)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(session_id)
    .bind(org_id.as_uuid())
    .bind(user_id)
    .bind(&title)
    .bind(req.page_scope.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(session_id)
}

async fn persist_interaction(
    state: &AppState,
    org_id: Uuid,
    session_id: Uuid,
    user_id: Uuid,
    interaction_id: Uuid,
    content: &str,
    citations: &[crate::types::Citation],
    tool_trace: &[crate::types::ToolTraceEntry],
    usage: &crate::types::TokenUsage,
    request_id: &str,
) -> Result<(), AppError> {
    let citations_json = serde_json::to_value(citations).unwrap_or(json!([]));
    let trace_json = serde_json::to_value(tool_trace).unwrap_or(json!([]));

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, companyos_tenancy::OrgId::new(org_id))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_interaction
            (id, org_id, session_id, user_id, role, content, citations, tool_trace,
             model, prompt_template_version, input_tokens, output_tokens, latency_ms,
             cost_estimate_minor, currency)
        VALUES ($1, $2, $3, $4, 'assistant', $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        "#,
    )
    .bind(interaction_id)
    .bind(org_id)
    .bind(session_id)
    .bind(user_id)
    .bind(content)
    .bind(citations_json)
    .bind(trace_json)
    .bind(&usage.model)
    .bind(&usage.prompt_template_version)
    .bind(usage.input_tokens as i32)
    .bind(usage.output_tokens as i32)
    .bind(usage.latency_ms as i32)
    .bind(usage.cost_estimate_minor)
    .bind(&usage.currency)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query("UPDATE ai_session SET updated_at = now() WHERE id = $1")
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}
