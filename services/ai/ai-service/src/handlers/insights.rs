//! GET/POST /api/v1/ai/insights — propose-only observation cards.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_ids::new_uuid_v7;

use crate::auth::AuthCtx;
use crate::handlers::common::{enforce_perm, extract_bearer, load_settings, resolve_principal};
use crate::handlers::insights_agents::{list_persisted_insights, run_insight_agents};
use crate::retrieval::{hybrid_retrieve, RetrievalQuery};
use crate::state::AppState;
use crate::types::{
    Citation, InsightObservation, InsightsRefreshResponse, InsightsResponse,
};
use companyos_errors::AppError;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ai/insights", get(get_insights))
        .route("/api/v1/ai/insights/refresh", post(refresh_insights))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/insights",
    responses((status = 200, body = InsightsResponse)),
    tag = "ai"
)]
pub async fn get_insights(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
) -> Result<Json<InsightsResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let bearer = extract_bearer(&headers).unwrap_or_default();

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_insights_read(), &request_id)?;

    let settings = load_settings(&state, org_id, &request_id).await?;
    if !settings.modules_enabled.insights {
        return Ok(Json(InsightsResponse {
            observations: Vec::new(),
            empty_reason: Some("insights module disabled".into()),
        }));
    }

    // Prefer persisted agent insights (Phase 3.5).
    let persisted = list_persisted_insights(&state, org_id, &request_id).await?;
    if !persisted.is_empty() {
        return Ok(Json(InsightsResponse {
            observations: persisted,
            empty_reason: None,
        }));
    }

    // Fall back to live retrieval / fixtures (Phase 1.9 compatible).
    let org_public = org_id.to_public().as_str();
    let citations = if bearer.is_empty() {
        Vec::new()
    } else {
        let query = RetrievalQuery::new(Some(&org_public), "overdue invoice open deal")?;
        hybrid_retrieve(&state, &auth, query, &bearer)
            .await
            .unwrap_or_default()
    };

    if citations.is_empty() {
        return Ok(Json(fixture_observations()));
    }

    let observations = citations
        .iter()
        .take(3)
        .map(|c| InsightObservation {
            id: new_uuid_v7().to_string(),
            title: format!("Review {}", c.title),
            body: format!(
                "Record {} ({}) may need attention based on workspace search.",
                c.record_type, c.record_id
            ),
            evidence: vec![c.clone()],
            suggested_action: Some("Review in workspace".into()),
            estimate: true,
            insight_type: Some(match c.record_type.as_str() {
                "deal" => "stale_deal".into(),
                "invoice" => "overdue_invoice".into(),
                "contract" => "upcoming_renewal".into(),
                _ => "workspace_signal".into(),
            }),
            status: Some("open".into()),
            suggested_action_detail: None,
            proposal_id: None,
        })
        .collect();

    Ok(Json(InsightsResponse {
        observations,
        empty_reason: None,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/insights/refresh",
    responses((status = 200, body = InsightsRefreshResponse)),
    tag = "ai"
)]
pub async fn refresh_insights(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
) -> Result<Json<InsightsRefreshResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;
    let bearer = extract_bearer(&headers).unwrap_or_default();

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_insights_read(), &request_id)?;

    let settings = load_settings(&state, org_id, &request_id).await?;
    if !settings.modules_enabled.insights {
        return Ok(Json(InsightsRefreshResponse {
            created: 0,
            observations: Vec::new(),
            pending_proposals: Vec::new(),
        }));
    }

    let result = run_insight_agents(&state, org_id, user_id, &bearer, &request_id).await?;
    Ok(Json(InsightsRefreshResponse {
        created: result.observations.len() as u32,
        observations: result.observations,
        pending_proposals: result.pending_proposals,
    }))
}

fn fixture_observations() -> InsightsResponse {
    let evidence = [
        Citation {
            record_type: "invoice".into(),
            record_id: "inv_fixture".into(),
            title: "Overdue invoice".into(),
            href: None,
            snippet: Some("Balance outstanding".into()),
        },
        Citation {
            record_type: "deal".into(),
            record_id: "dl_fixture".into(),
            title: "Stalled deal".into(),
            href: None,
            snippet: Some("No activity 14 days".into()),
        },
        Citation {
            record_type: "task".into(),
            record_id: "tsk_fixture".into(),
            title: "Blocked task".into(),
            href: None,
            snippet: Some("Waiting on approval".into()),
        },
    ];

    InsightsResponse {
        observations: vec![
            InsightObservation {
                id: new_uuid_v7().to_string(),
                title: "Overdue invoices".into(),
                body: "Several invoices appear overdue — consider follow-up.".into(),
                evidence: vec![evidence[0].clone()],
                suggested_action: Some("draft_follow_up_activity".into()),
                estimate: true,
                insight_type: Some("overdue_invoice".into()),
                status: Some("open".into()),
                suggested_action_detail: None,
                proposal_id: None,
            },
            InsightObservation {
                id: new_uuid_v7().to_string(),
                title: "Stalled deals".into(),
                body: "Open deals without recent activity detected.".into(),
                evidence: vec![evidence[1].clone()],
                suggested_action: Some("create_deal_note".into()),
                estimate: true,
                insight_type: Some("stale_deal".into()),
                status: Some("open".into()),
                suggested_action_detail: None,
                proposal_id: None,
            },
            InsightObservation {
                id: new_uuid_v7().to_string(),
                title: "Task backlog".into(),
                body: "Blocked tasks may delay delivery.".into(),
                evidence: vec![evidence[2].clone()],
                suggested_action: Some("create_task".into()),
                estimate: true,
                insight_type: Some("workspace_signal".into()),
                status: Some("open".into()),
                suggested_action_detail: None,
                proposal_id: None,
            },
        ],
        empty_reason: None,
    }
}
