//! Inline suggestion chips for page context.

use axum::extract::{Query, State};
use axum::{Json, Router};
use axum::routing::get;
use companyos_authz::perms;
use companyos_ids::new_uuid_v7;
use serde::Deserialize;
use serde_json::json;

use crate::auth::AuthCtx;
use crate::handlers::common::{resolve_principal, enforce_perm};
use crate::state::AppState;
use crate::tools::{persist_proposal, ProposalDraft};
use crate::types::{SuggestionChip, SuggestionsResponse};
use companyos_errors::AppError;

#[derive(Debug, Deserialize)]
pub struct SuggestionsQuery {
    pub page_scope: Option<String>,
    pub record_id: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/ai/suggestions", get(get_suggestions))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/suggestions",
    params(
        ("page_scope" = Option<String>, Query),
        ("record_id" = Option<String>, Query),
    ),
    responses((status = 200, body = SuggestionsResponse)),
    tag = "ai"
)]
pub async fn get_suggestions(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<SuggestionsQuery>,
) -> Result<Json<SuggestionsResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_copilot_use(), &request_id)?;

    let scope = q.page_scope.as_deref().unwrap_or("");
    let record_id = q.record_id.as_deref().unwrap_or("");
    let mut chips = Vec::new();

    if scope.starts_with("deal") || record_id.starts_with("dl_") {
        let draft = ProposalDraft {
            tool_name: "draft_follow_up_activity".into(),
            action_type: "draft_follow_up_activity".into(),
            command: json!({
                "subject": "Next best action",
                "body": "Schedule follow-up for this deal",
                "deal_id": record_id,
            }),
            rendered_diff: "+ Draft follow-up activity".into(),
            citations: Vec::new(),
            domain_path: "/api/v1/sales/activities".into(),
            domain_method: "POST".into(),
            domain_body: json!({
                "kind": "email",
                "subject": "Next best action",
                "deal_id": record_id,
            }),
        };
        let proposal_id = persist_proposal(
            &state,
            org_id.as_uuid(),
            user_id,
            None,
            &draft,
            &request_id,
        )
        .await?;
        chips.push(SuggestionChip {
            id: new_uuid_v7().to_string(),
            label: "Draft follow-up".into(),
            action_type: "draft_follow_up_activity".into(),
            proposal_id: Some(proposal_id.to_string()),
        });
    } else if scope.contains("invoice") || record_id.starts_with("inv_") {
        let draft = ProposalDraft {
            tool_name: "draft_follow_up_activity".into(),
            action_type: "draft_follow_up_activity".into(),
            command: json!({
                "subject": "Payment reminder",
                "body": "Follow up on overdue invoice",
            }),
            rendered_diff: "+ Draft payment reminder".into(),
            citations: Vec::new(),
            domain_path: "/api/v1/sales/activities".into(),
            domain_method: "POST".into(),
            domain_body: json!({
                "kind": "email",
                "subject": "Payment reminder",
            }),
        };
        let proposal_id = persist_proposal(
            &state,
            org_id.as_uuid(),
            user_id,
            None,
            &draft,
            &request_id,
        )
        .await?;
        chips.push(SuggestionChip {
            id: new_uuid_v7().to_string(),
            label: "Send payment reminder".into(),
            action_type: "draft_follow_up_activity".into(),
            proposal_id: Some(proposal_id.to_string()),
        });
    } else if scope.contains("task") || record_id.starts_with("tsk_") {
        let draft = ProposalDraft {
            tool_name: "create_task".into(),
            action_type: "create_task".into(),
            command: json!({
                "project_id": "prj_placeholder",
                "title": "Subtask from brief",
            }),
            rendered_diff: "+ Break down into subtasks".into(),
            citations: Vec::new(),
            domain_path: "/api/v1/operations/tasks".into(),
            domain_method: "POST".into(),
            domain_body: json!({
                "project_id": "prj_placeholder",
                "title": "Subtask from brief",
            }),
        };
        let proposal_id = persist_proposal(
            &state,
            org_id.as_uuid(),
            user_id,
            None,
            &draft,
            &request_id,
        )
        .await?;
        chips.push(SuggestionChip {
            id: new_uuid_v7().to_string(),
            label: "Break down task".into(),
            action_type: "create_task".into(),
            proposal_id: Some(proposal_id.to_string()),
        });
    }

    Ok(Json(SuggestionsResponse { chips }))
}
