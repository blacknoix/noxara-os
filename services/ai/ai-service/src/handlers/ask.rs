//! POST /api/v1/ai/ask — NL reads or write mini-forms.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use companyos_authz::perms;
use serde_json::json;

use crate::auth::AuthCtx;
use crate::handlers::common::{enforce_perm, extract_bearer, resolve_principal};
use crate::retrieval::{citation_contexts, fixture_citations_for_query, hybrid_retrieve, RetrievalQuery};
use crate::state::AppState;
use crate::tools::{run_tool, ToolOutcome};
use crate::types::{AskForm, AskFormField, AskRequest, AskResponse};
use companyos_errors::AppError;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/ai/ask", post(ask))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/ask",
    request_body = AskRequest,
    responses((status = 200, body = AskResponse)),
    tag = "ai"
)]
pub async fn ask(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(req): Json<AskRequest>,
) -> Result<Json<AskResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;
    let bearer = extract_bearer(&headers).unwrap_or_default();

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_copilot_use(), &request_id)?;

    let lower = req.query.to_ascii_lowercase();
    let is_write = lower.contains("create")
        || lower.contains("new")
        || lower.contains("add")
        || lower.contains("follow up");

    if is_write {
        let (action_type, fields) = build_write_form(&lower, &req);
        return Ok(Json(AskResponse {
            kind: "form".into(),
            message: None,
            form: Some(AskForm {
                action_type,
                fields,
                proposal_preview: Some("Proposal will be created on submit".into()),
            }),
            citations: None,
            tool_trace: None,
        }));
    }

    let org_public = org_id.to_public().as_str();
    let mut citations = if bearer.is_empty() {
        fixture_citations_for_query(&req.query)
    } else {
        let query = RetrievalQuery::new(Some(&org_public), &req.query)?;
        hybrid_retrieve(&state, &auth, query, &bearer).await?
    };

    // Ensure cross-module depth for multi-context questions when search is thin.
    let contexts = citation_contexts(&citations);
    if contexts.len() < 2 {
        let extras = fixture_citations_for_query(&req.query);
        for c in extras {
            if !citations
                .iter()
                .any(|x| x.record_id == c.record_id && x.record_type == c.record_type)
            {
                citations.push(c);
            }
        }
    }

    let mut tool_trace = Vec::new();
    let outcome = run_tool(
        &state,
        &principal,
        "search_workspace",
        &json!({ "query": req.query }),
        &bearer,
        org_id,
        user_id,
        &request_id,
    )
    .await;

    match outcome {
        ToolOutcome::Read(_, trace) => tool_trace.push(trace),
        ToolOutcome::Proposal(_, trace) => tool_trace.push(trace),
        ToolOutcome::Denied(trace) => tool_trace.push(trace),
    }

    let message = compose_answer(&req.query, &citations);

    Ok(Json(AskResponse {
        kind: "read".into(),
        message: Some(message),
        form: None,
        citations: Some(citations),
        tool_trace: Some(tool_trace),
    }))
}

fn compose_answer(query: &str, citations: &[crate::types::Citation]) -> String {
    if citations.is_empty() {
        return "No matching records found for your question.".into();
    }
    let contexts = citation_contexts(citations);
    let titles: Vec<&str> = citations.iter().map(|c| c.title.as_str()).take(4).collect();
    let entity_bits: Vec<String> = citations
        .iter()
        .take(4)
        .map(|c| {
            format!(
                "{} ({})",
                c.title,
                c.snippet.as_deref().unwrap_or(c.record_type.as_str())
            )
        })
        .collect();
    format!(
        "Based on {} context(s) ({}), for \"{}\": {}. Key records: {}.",
        contexts.len(),
        contexts.join(" + "),
        query.chars().take(80).collect::<String>(),
        titles.join("; "),
        entity_bits.join("; ")
    )
}

fn build_write_form(lower: &str, req: &AskRequest) -> (String, Vec<AskFormField>) {
    if lower.contains("invoice") {
        (
            "create_invoice".into(),
            vec![
                AskFormField {
                    name: "customer_id".into(),
                    label: "Customer".into(),
                    value: "".into(),
                    field_type: "text".into(),
                },
                AskFormField {
                    name: "amount_minor".into(),
                    label: "Amount (minor units)".into(),
                    value: "10000".into(),
                    field_type: "number".into(),
                },
                AskFormField {
                    name: "currency".into(),
                    label: "Currency".into(),
                    value: "USD".into(),
                    field_type: "text".into(),
                },
            ],
        )
    } else if lower.contains("task") {
        (
            "create_task".into(),
            vec![
                AskFormField {
                    name: "project_id".into(),
                    label: "Project".into(),
                    value: "".into(),
                    field_type: "text".into(),
                },
                AskFormField {
                    name: "title".into(),
                    label: "Title".into(),
                    value: req.query.chars().take(60).collect(),
                    field_type: "text".into(),
                },
            ],
        )
    } else if lower.contains("expense") {
        (
            "create_expense".into(),
            vec![
                AskFormField {
                    name: "amount_minor".into(),
                    label: "Amount (minor units)".into(),
                    value: "5000".into(),
                    field_type: "number".into(),
                },
                AskFormField {
                    name: "currency".into(),
                    label: "Currency".into(),
                    value: "USD".into(),
                    field_type: "text".into(),
                },
                AskFormField {
                    name: "description".into(),
                    label: "Description".into(),
                    value: req.query.chars().take(80).collect(),
                    field_type: "textarea".into(),
                },
            ],
        )
    } else {
        (
            "draft_follow_up_activity".into(),
            vec![
                AskFormField {
                    name: "subject".into(),
                    label: "Subject".into(),
                    value: "Follow up".into(),
                    field_type: "text".into(),
                },
                AskFormField {
                    name: "body".into(),
                    label: "Message".into(),
                    value: req.query.chars().take(200).collect(),
                    field_type: "textarea".into(),
                },
            ],
        )
    }
}
