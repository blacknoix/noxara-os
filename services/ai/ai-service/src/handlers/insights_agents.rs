//! Proactive insight agents — propose-only cards across sales + finance (+ more).
//!
//! HITL: agents may create `ai_insight` rows and pending `ai_proposal` drafts.
//! They NEVER mutate CRM/finance domain tables.

use chrono::Utc;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::gateway_client::forward_user_request;
use crate::state::AppState;
use crate::tools::{persist_proposal, ProposalDraft};
use crate::types::{Citation, InsightObservation};
use axum::http::{Method, StatusCode};

#[derive(Debug, Clone)]
pub struct AgentInsight {
    pub insight_type: String,
    pub title: String,
    pub body: String,
    pub citations: Vec<Citation>,
    pub suggested_action: Option<Value>,
    pub proposal: Option<ProposalDraft>,
}

#[derive(Debug, Default)]
pub struct RefreshResult {
    pub observations: Vec<InsightObservation>,
    pub pending_proposals: Vec<String>,
}

/// Run cross-module insight agents and persist propose-only rows.
pub async fn run_insight_agents(
    state: &AppState,
    org_id: OrgId,
    user_id: Uuid,
    bearer: &str,
    request_id: &str,
) -> Result<RefreshResult, AppError> {
    let mut drafts = Vec::new();
    drafts.extend(stale_deal_agent(state, org_id, user_id, bearer, request_id).await?);
    drafts.extend(overdue_invoice_agent(state, org_id, user_id, bearer, request_id).await?);
    drafts.extend(upcoming_renewal_agent(state, org_id, user_id, bearer, request_id).await?);

    if drafts.is_empty() {
        drafts = fixture_agent_insights();
    }

    let mut result = RefreshResult::default();
    for draft in drafts {
        let (obs, proposal_id) =
            persist_insight(state, org_id, user_id, &draft, request_id).await?;
        if let Some(pid) = proposal_id {
            result.pending_proposals.push(pid);
        }
        result.observations.push(obs);
    }
    Ok(result)
}

async fn stale_deal_agent(
    state: &AppState,
    org_id: OrgId,
    user_id: Uuid,
    bearer: &str,
    request_id: &str,
) -> Result<Vec<AgentInsight>, AppError> {
    let (status, body) = forward_user_request(
        state,
        bearer,
        Method::GET,
        "/api/v1/sales/deals?status=open",
        None,
        false,
        user_id,
        request_id,
    )
    .await
    .unwrap_or((StatusCode::SERVICE_UNAVAILABLE, Value::Null));

    if !status.is_success() {
        return Ok(Vec::new());
    }

    let items = body
        .get("items")
        .or_else(|| body.get("deals"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for deal in items.into_iter().take(3) {
        let deal_id = deal
            .get("id")
            .or_else(|| deal.get("public_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("dl_unknown");
        let title = deal
            .get("name")
            .or_else(|| deal.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("Open deal");
        let citation = Citation {
            record_type: "deal".into(),
            record_id: deal_id.to_string(),
            title: title.to_string(),
            href: Some(format!("/sales/deals?q={deal_id}")),
            snippet: Some("No recent activity detected".into()),
        };
        let suggested = json!({
            "action_type": "create_deal_note",
            "tool_name": "create_deal_note",
            "args": {
                "deal_id": deal_id,
                "body": format!("AI suggestion: follow up on stalled deal {title}"),
            }
        });
        let proposal = ProposalDraft {
            tool_name: "create_deal_note".into(),
            action_type: "create_deal_note".into(),
            command: suggested["args"].clone(),
            rendered_diff: format!("+ Deal note for {title} (propose only)"),
            citations: vec![citation.clone()],
            domain_path: "/api/v1/sales/activities".into(),
            domain_method: "POST".into(),
            domain_body: json!({
                "kind": "note",
                "body": format!("AI suggestion: follow up on stalled deal {title}"),
                "deal_id": deal_id,
            }),
        };
        out.push(AgentInsight {
            insight_type: "stale_deal".into(),
            title: format!("Stale deal: {title}"),
            body: format!(
                "Open deal {title} has no recent activity. Proposed follow-up note (human confirm required)."
            ),
            citations: vec![citation],
            suggested_action: Some(suggested),
            proposal: Some(proposal),
        });
        let _ = org_id;
    }
    Ok(out)
}

async fn overdue_invoice_agent(
    state: &AppState,
    org_id: OrgId,
    user_id: Uuid,
    bearer: &str,
    request_id: &str,
) -> Result<Vec<AgentInsight>, AppError> {
    let (status, body) = forward_user_request(
        state,
        bearer,
        Method::GET,
        "/api/v1/finance/invoices?status=overdue",
        None,
        false,
        user_id,
        request_id,
    )
    .await
    .unwrap_or((StatusCode::SERVICE_UNAVAILABLE, Value::Null));

    if !status.is_success() {
        return Ok(Vec::new());
    }

    let items = body
        .get("items")
        .or_else(|| body.get("invoices"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for inv in items.into_iter().take(3) {
        let inv_id = inv
            .get("id")
            .or_else(|| inv.get("public_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("inv_unknown");
        let title = inv
            .get("number")
            .or_else(|| inv.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("Overdue invoice");
        let citation = Citation {
            record_type: "invoice".into(),
            record_id: inv_id.to_string(),
            title: title.to_string(),
            href: Some(format!("/finance/invoices/{inv_id}")),
            snippet: Some("Balance outstanding / overdue".into()),
        };
        let suggested = json!({
            "action_type": "draft_follow_up_activity",
            "tool_name": "draft_follow_up_activity",
            "args": {
                "subject": format!("Payment reminder for {title}"),
                "body": "AI draft payment reminder — confirm before send.",
            }
        });
        let proposal = ProposalDraft {
            tool_name: "draft_follow_up_activity".into(),
            action_type: "draft_follow_up_activity".into(),
            command: suggested["args"].clone(),
            rendered_diff: format!("+ Payment reminder draft for {title}"),
            citations: vec![citation.clone()],
            domain_path: "/api/v1/sales/activities".into(),
            domain_method: "POST".into(),
            domain_body: json!({
                "kind": "email",
                "subject": format!("Payment reminder for {title}"),
                "body": "AI draft payment reminder — confirm before send.",
            }),
        };
        out.push(AgentInsight {
            insight_type: "overdue_invoice".into(),
            title: format!("Overdue invoice: {title}"),
            body: format!(
                "Invoice {title} appears overdue. Draft reminder proposed only — no domain write."
            ),
            citations: vec![citation],
            suggested_action: Some(suggested),
            proposal: Some(proposal),
        });
        let _ = org_id;
    }
    Ok(out)
}

async fn upcoming_renewal_agent(
    state: &AppState,
    org_id: OrgId,
    user_id: Uuid,
    bearer: &str,
    request_id: &str,
) -> Result<Vec<AgentInsight>, AppError> {
    // Contracts may live under sales; degrade gracefully when unavailable.
    let (status, body) = forward_user_request(
        state,
        bearer,
        Method::GET,
        "/api/v1/sales/contracts?status=active",
        None,
        false,
        user_id,
        request_id,
    )
    .await
    .unwrap_or((StatusCode::SERVICE_UNAVAILABLE, Value::Null));

    if !status.is_success() {
        return Ok(Vec::new());
    }

    let items = body
        .get("items")
        .or_else(|| body.get("contracts"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for c in items.into_iter().take(2) {
        let cid = c
            .get("id")
            .or_else(|| c.get("public_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("sct_unknown");
        let title = c
            .get("name")
            .or_else(|| c.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("Contract");
        let citation = Citation {
            record_type: "contract".into(),
            record_id: cid.to_string(),
            title: title.to_string(),
            href: Some(format!("/sales/contracts/{cid}")),
            snippet: Some("Upcoming renewal window".into()),
        };
        out.push(AgentInsight {
            insight_type: "upcoming_renewal".into(),
            title: format!("Upcoming renewal: {title}"),
            body: format!(
                "Contract {title} is approaching renewal. Review commercially — no auto-write."
            ),
            citations: vec![citation],
            suggested_action: Some(json!({
                "action_type": "review_renewal",
                "args": { "contract_id": cid }
            })),
            proposal: None,
        });
        let _ = org_id;
    }
    Ok(out)
}

/// Fixture agents used when gateway domain APIs are unavailable (CI).
pub fn fixture_agent_insights() -> Vec<AgentInsight> {
    let deal = Citation {
        record_type: "deal".into(),
        record_id: "dl_acme_stale".into(),
        title: "Acme Enterprise".into(),
        href: Some("/sales/deals?q=dl_acme_stale".into()),
        snippet: Some("No activity 18 days".into()),
    };
    let invoice = Citation {
        record_type: "invoice".into(),
        record_id: "inv_acme_1001".into(),
        title: "INV-1001 Acme".into(),
        href: Some("/finance/invoices/inv_acme_1001".into()),
        snippet: Some("Overdue 12 days".into()),
    };
    let contract = Citation {
        record_type: "contract".into(),
        record_id: "sct_northwind".into(),
        title: "Northwind Annual".into(),
        href: Some("/sales/contracts/sct_northwind".into()),
        snippet: Some("Renews in 28 days".into()),
    };

    let deal_proposal = ProposalDraft {
        tool_name: "create_deal_note".into(),
        action_type: "create_deal_note".into(),
        command: json!({
            "deal_id": "dl_acme_stale",
            "body": "AI suggestion: follow up on stalled Acme Enterprise deal",
        }),
        rendered_diff: "+ Deal note for Acme Enterprise (propose only)".into(),
        citations: vec![deal.clone()],
        domain_path: "/api/v1/sales/activities".into(),
        domain_method: "POST".into(),
        domain_body: json!({
            "kind": "note",
            "body": "AI suggestion: follow up on stalled Acme Enterprise deal",
            "deal_id": "dl_acme_stale",
        }),
    };
    let invoice_proposal = ProposalDraft {
        tool_name: "draft_follow_up_activity".into(),
        action_type: "draft_follow_up_activity".into(),
        command: json!({
            "subject": "Payment reminder for INV-1001 Acme",
            "body": "AI draft payment reminder — confirm before send.",
        }),
        rendered_diff: "+ Payment reminder draft for INV-1001 Acme".into(),
        citations: vec![invoice.clone()],
        domain_path: "/api/v1/sales/activities".into(),
        domain_method: "POST".into(),
        domain_body: json!({
            "kind": "email",
            "subject": "Payment reminder for INV-1001 Acme",
            "body": "AI draft payment reminder — confirm before send.",
        }),
    };

    vec![
        AgentInsight {
            insight_type: "stale_deal".into(),
            title: "Stale deal: Acme Enterprise".into(),
            body: "Open deal Acme Enterprise has no recent activity. Proposed follow-up note (human confirm required).".into(),
            citations: vec![deal],
            suggested_action: Some(json!({
                "action_type": "create_deal_note",
                "tool_name": "create_deal_note",
            })),
            proposal: Some(deal_proposal),
        },
        AgentInsight {
            insight_type: "overdue_invoice".into(),
            title: "Overdue invoice: INV-1001 Acme".into(),
            body: "Invoice INV-1001 Acme appears overdue. Draft reminder proposed only — no domain write.".into(),
            citations: vec![invoice],
            suggested_action: Some(json!({
                "action_type": "draft_follow_up_activity",
                "tool_name": "draft_follow_up_activity",
            })),
            proposal: Some(invoice_proposal),
        },
        AgentInsight {
            insight_type: "upcoming_renewal".into(),
            title: "Upcoming renewal: Northwind Annual".into(),
            body: "Contract Northwind Annual is approaching renewal. Review commercially — no auto-write.".into(),
            citations: vec![contract],
            suggested_action: Some(json!({
                "action_type": "review_renewal",
            })),
            proposal: None,
        },
    ]
}

async fn persist_insight(
    state: &AppState,
    org_id: OrgId,
    user_id: Uuid,
    draft: &AgentInsight,
    request_id: &str,
) -> Result<(InsightObservation, Option<String>), AppError> {
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::AiInsight, id).as_str();
    let citations_json = serde_json::to_value(&draft.citations).unwrap_or(json!([]));
    let suggested = draft.suggested_action.clone().unwrap_or(Value::Null);

    let mut proposal_id: Option<Uuid> = None;
    if let Some(ref prop) = draft.proposal {
        let pid =
            persist_proposal(state, org_id.as_uuid(), user_id, None, prop, request_id).await?;
        proposal_id = Some(pid);
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_insight (
            id, org_id, public_id, insight_type, title, body,
            citations, suggested_action, status, created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'open',$9,$9)
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(&public_id)
    .bind(&draft.insight_type)
    .bind(&draft.title)
    .bind(&draft.body)
    .bind(&citations_json)
    .bind(&suggested)
    .bind(Utc::now())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let action_type = draft
        .suggested_action
        .as_ref()
        .and_then(|v| v.get("action_type"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    Ok((
        InsightObservation {
            id: public_id,
            title: draft.title.clone(),
            body: draft.body.clone(),
            evidence: draft.citations.clone(),
            suggested_action: action_type,
            estimate: true,
            insight_type: Some(draft.insight_type.clone()),
            status: Some("open".into()),
            suggested_action_detail: draft.suggested_action.clone(),
            proposal_id: proposal_id.map(|p| p.to_string()),
        },
        proposal_id.map(|p| p.to_string()),
    ))
}

pub async fn list_persisted_insights(
    state: &AppState,
    org_id: OrgId,
    request_id: &str,
) -> Result<Vec<InsightObservation>, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    type InsightRow = (
        Uuid,
        String,
        String,
        String,
        String,
        Value,
        Option<Value>,
        String,
    );
    let rows: Vec<InsightRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, insight_type, title, body, citations, suggested_action, status
        FROM ai_insight
        WHERE org_id = $1 AND status = 'open'
        ORDER BY created_at DESC
        LIMIT 50
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(rows
        .into_iter()
        .map(
            |(_id, public_id, insight_type, title, body, citations, suggested, status)| {
                let evidence: Vec<Citation> = serde_json::from_value(citations).unwrap_or_default();
                let action_type = suggested
                    .as_ref()
                    .and_then(|v| v.get("action_type"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                InsightObservation {
                    id: public_id,
                    title,
                    body,
                    evidence,
                    suggested_action: action_type,
                    estimate: true,
                    insight_type: Some(insight_type),
                    status: Some(status),
                    suggested_action_detail: suggested,
                    proposal_id: None,
                }
            },
        )
        .collect())
}
