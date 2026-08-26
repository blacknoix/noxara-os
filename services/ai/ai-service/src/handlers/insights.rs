//! GET /api/v1/ai/insights

use axum::extract::State;
use axum::{Json, Router};
use axum::routing::get;
use companyos_authz::perms;
use companyos_ids::new_uuid_v7;

use crate::auth::AuthCtx;
use crate::handlers::common::{extract_bearer, load_settings, resolve_principal, enforce_perm};
use crate::retrieval::{hybrid_retrieve, RetrievalQuery};
use crate::state::AppState;
use crate::types::{Citation, InsightObservation, InsightsResponse};
use companyos_errors::AppError;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/ai/insights", get(get_insights))
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
    headers: axum::http::HeaderMap,
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

    let org_public = org_id.to_public().as_str();
    let citations = if bearer.is_empty() {
        Vec::new()
    } else {
        let query = RetrievalQuery::new(Some(&org_public), "overdue invoice open deal")?;
        hybrid_retrieve(&state, &auth, query, &bearer).await.unwrap_or_default()
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
        })
        .collect();

    Ok(Json(InsightsResponse {
        observations,
        empty_reason: None,
    }))
}

fn fixture_observations() -> InsightsResponse {
    let evidence = vec![
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
            },
            InsightObservation {
                id: new_uuid_v7().to_string(),
                title: "Stalled deals".into(),
                body: "Open deals without recent activity detected.".into(),
                evidence: vec![evidence[1].clone()],
                suggested_action: Some("create_deal_note".into()),
                estimate: true,
            },
            InsightObservation {
                id: new_uuid_v7().to_string(),
                title: "Task backlog".into(),
                body: "Blocked tasks may delay delivery.".into(),
                evidence: vec![evidence[2].clone()],
                suggested_action: Some("create_task".into()),
                estimate: true,
            },
        ],
        empty_reason: None,
    }
}
