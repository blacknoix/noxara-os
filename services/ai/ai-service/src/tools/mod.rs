//! Tool registry — authz decide() before every tool; writes return proposals only.

use std::time::Instant;

use axum::http::Method;
use companyos_authz::perms;
use companyos_authz::PermissionId;
use companyos_ids::new_uuid_v7;
use companyos_tenancy::OrgId;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::gateway_client::forward_user_request;
use crate::principal::decide_traced;
use crate::state::AppState;
use crate::types::{Citation, ToolTraceEntry};
use companyos_authz::Principal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Write,
}

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub permission: fn() -> PermissionId,
    pub kind: ToolKind,
    pub action_type: &'static str,
}

pub static ALL_TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "search_workspace",
        description: "Search workspace records",
        permission: perms::platform_search_read,
        kind: ToolKind::Read,
        action_type: "search",
    },
    ToolDef {
        name: "get_deal",
        description: "Get deal by id",
        permission: perms::sales_deal_read,
        kind: ToolKind::Read,
        action_type: "read_deal",
    },
    ToolDef {
        name: "get_invoice",
        description: "Get invoice by id",
        permission: perms::finance_invoice_read,
        kind: ToolKind::Read,
        action_type: "read_invoice",
    },
    ToolDef {
        name: "list_overdue_invoices",
        description: "List overdue invoices",
        permission: perms::finance_invoice_read,
        kind: ToolKind::Read,
        action_type: "list_overdue_invoices",
    },
    ToolDef {
        name: "get_task",
        description: "Get task by id",
        permission: perms::operations_task_read,
        kind: ToolKind::Read,
        action_type: "read_task",
    },
    ToolDef {
        name: "list_open_deals",
        description: "List open deals",
        permission: perms::sales_deal_read,
        kind: ToolKind::Read,
        action_type: "list_open_deals",
    },
    ToolDef {
        name: "create_invoice",
        description: "Propose creating an invoice",
        permission: perms::finance_invoice_create,
        kind: ToolKind::Write,
        action_type: "create_invoice",
    },
    ToolDef {
        name: "create_task",
        description: "Propose creating a task",
        permission: perms::operations_task_create,
        kind: ToolKind::Write,
        action_type: "create_task",
    },
    ToolDef {
        name: "create_expense",
        description: "Propose creating an expense",
        permission: perms::finance_expense_create,
        kind: ToolKind::Write,
        action_type: "create_expense",
    },
    ToolDef {
        name: "draft_follow_up_activity",
        description: "Propose a follow-up activity",
        permission: perms::sales_activity_create,
        kind: ToolKind::Write,
        action_type: "draft_follow_up_activity",
    },
    ToolDef {
        name: "create_deal_note",
        description: "Propose a deal note activity",
        permission: perms::sales_activity_create,
        kind: ToolKind::Write,
        action_type: "create_deal_note",
    },
];

pub fn find_tool(name: &str) -> Option<&'static ToolDef> {
    ALL_TOOLS.iter().find(|t| t.name == name)
}

#[derive(Debug, Clone)]
pub struct ProposalDraft {
    pub tool_name: String,
    pub action_type: String,
    pub command: Value,
    pub rendered_diff: String,
    pub citations: Vec<Citation>,
    pub domain_path: String,
    pub domain_method: String,
    pub domain_body: Value,
}

#[derive(Debug)]
pub enum ToolOutcome {
    Read(Value, ToolTraceEntry),
    Proposal(ProposalDraft, ToolTraceEntry),
    Denied(ToolTraceEntry),
}

pub async fn run_tool(
    state: &AppState,
    principal: &Principal,
    tool_name: &str,
    args: &Value,
    bearer: &str,
    org_id: OrgId,
    user_id: Uuid,
    request_id: &str,
) -> ToolOutcome {
    let start = Instant::now();
    let tool = find_tool(tool_name);
    let args_summary = args.to_string().chars().take(120).collect::<String>();

    let Some(def) = tool else {
        let trace = ToolTraceEntry {
            tool_name: tool_name.to_string(),
            permission: "unknown".into(),
            decision: "deny".into(),
            reason: "unknown_tool".into(),
            args_summary,
            duration_ms: start.elapsed().as_millis() as u64,
        };
        return ToolOutcome::Denied(trace);
    };

    let perm = (def.permission)();
    let mut trace = decide_traced(principal, perm, def.name, &args_summary);
    trace.duration_ms = start.elapsed().as_millis() as u64;

    if trace.decision == "deny" {
        return ToolOutcome::Denied(trace);
    }

    match def.kind {
        ToolKind::Read => {
            let value = match def.name {
                "search_workspace" => {
                    let q = args
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let org_public = org_id.to_public().as_str();
                    let url = format!(
                        "/api/v1/search/query?q={}&org_id={}",
                        urlencoding::encode(q),
                        org_public
                    );
                    let (status, body) = forward_user_request(
                        state,
                        bearer,
                        Method::GET,
                        &url,
                        None,
                        false,
                        user_id,
                        request_id,
                    )
                    .await
                    .unwrap_or((axum::http::StatusCode::INTERNAL_SERVER_ERROR, Value::Null));
                    json!({ "status": status.as_u16(), "body": body })
                }
                "get_deal" => {
                    let id = args.get("deal_id").and_then(|v| v.as_str()).unwrap_or("");
                    read_get(state, bearer, user_id, request_id, &format!("/api/v1/sales/deals/{id}"))
                        .await
                }
                "get_invoice" => {
                    let id = args.get("invoice_id").and_then(|v| v.as_str()).unwrap_or("");
                    read_get(
                        state,
                        bearer,
                        user_id,
                        request_id,
                        &format!("/api/v1/finance/invoices/{id}"),
                    )
                    .await
                }
                "list_overdue_invoices" => {
                    read_get(
                        state,
                        bearer,
                        user_id,
                        request_id,
                        "/api/v1/finance/invoices?status=overdue",
                    )
                    .await
                }
                "get_task" => {
                    let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                    read_get(
                        state,
                        bearer,
                        user_id,
                        request_id,
                        &format!("/api/v1/operations/tasks/{id}"),
                    )
                    .await
                }
                "list_open_deals" => {
                    read_get(
                        state,
                        bearer,
                        user_id,
                        request_id,
                        "/api/v1/sales/deals?status=open",
                    )
                    .await
                }
                _ => Value::Null,
            };
            ToolOutcome::Read(value, trace)
        }
        ToolKind::Write => {
            let (command, rendered_diff, domain_path, domain_method, domain_body) =
                build_write_proposal(def.name, args);
            let draft = ProposalDraft {
                tool_name: def.name.to_string(),
                action_type: def.action_type.to_string(),
                command,
                rendered_diff,
                citations: Vec::new(),
                domain_path,
                domain_method,
                domain_body,
            };
            ToolOutcome::Proposal(draft, trace)
        }
    }
}

async fn read_get(
    state: &AppState,
    bearer: &str,
    user_id: Uuid,
    request_id: &str,
    path: &str,
) -> Value {
    let (_, body) = forward_user_request(
        state,
        bearer,
        Method::GET,
        path,
        None,
        false,
        user_id,
        request_id,
    )
    .await
    .unwrap_or((axum::http::StatusCode::INTERNAL_SERVER_ERROR, Value::Null));
    body
}

fn build_write_proposal(
    name: &str,
    args: &Value,
) -> (Value, String, String, String, Value) {
    match name {
        "create_invoice" => {
            let customer = args
                .get("customer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("cus_placeholder");
            let amount = args.get("amount_minor").and_then(|v| v.as_i64()).unwrap_or(0);
            let currency = args
                .get("currency")
                .and_then(|v| v.as_str())
                .unwrap_or("USD");
            let body = json!({
                "customer_id": customer,
                "currency": currency,
                "lines": [{
                    "description": "AI proposed line",
                    "quantity": 1,
                    "unit_price_minor": amount,
                }],
            });
            let diff = format!("+ Create invoice for {customer}\n  Total: {amount} {currency} minor");
            (
                body.clone(),
                diff,
                "/api/v1/finance/invoices".into(),
                "POST".into(),
                body,
            )
        }
        "create_task" => {
            let project = args
                .get("project_id")
                .and_then(|v| v.as_str())
                .unwrap_or("prj_placeholder");
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("AI proposed task");
            let body = json!({
                "project_id": project,
                "title": title,
            });
            let diff = format!("+ Create task: {title}");
            (
                body.clone(),
                diff,
                "/api/v1/operations/tasks".into(),
                "POST".into(),
                body,
            )
        }
        "create_expense" => {
            let amount = args.get("amount_minor").and_then(|v| v.as_i64()).unwrap_or(0);
            let currency = args
                .get("currency")
                .and_then(|v| v.as_str())
                .unwrap_or("USD");
            let description = args
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("AI proposed expense");
            let body = json!({
                "currency": currency,
                "amount_minor": amount,
                "description": description,
            });
            let diff = format!("+ Create expense: {description} ({amount} {currency} minor)");
            (
                body.clone(),
                diff,
                "/api/v1/finance/expenses".into(),
                "POST".into(),
                body,
            )
        }
        "draft_follow_up_activity" => {
            let deal_id = args.get("deal_id").and_then(|v| v.as_str());
            let subject = args
                .get("subject")
                .and_then(|v| v.as_str())
                .unwrap_or("Follow up");
            let body = json!({
                "kind": "email",
                "subject": subject,
                "body": args.get("body").and_then(|v| v.as_str()).unwrap_or("Follow-up draft"),
                "deal_id": deal_id,
            });
            let diff = format!("+ Draft follow-up: {subject}");
            (
                body.clone(),
                diff,
                "/api/v1/sales/activities".into(),
                "POST".into(),
                body,
            )
        }
        "create_deal_note" => {
            let deal_id = args.get("deal_id").and_then(|v| v.as_str());
            let note = args
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("Deal note");
            let body = json!({
                "kind": "note",
                "body": note,
                "deal_id": deal_id,
            });
            let diff = format!("+ Deal note: {note}");
            (
                body.clone(),
                diff,
                "/api/v1/sales/activities".into(),
                "POST".into(),
                body,
            )
        }
        _ => (
            json!({}),
            "+ Unknown write".into(),
            "/".into(),
            "POST".into(),
            json!({}),
        ),
    }
}

pub async fn persist_proposal(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    interaction_id: Option<Uuid>,
    draft: &ProposalDraft,
    request_id: &str,
) -> Result<Uuid, companyos_errors::AppError> {
    use companyos_errors::{AppError, ErrorCode};
    use companyos_tenancy::set_session_org_id;

    let id = new_uuid_v7();
    let citations_json = serde_json::to_value(&draft.citations).unwrap_or(json!([]));

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, OrgId::new(org_id))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_proposal
            (id, org_id, user_id, interaction_id, tool_name, action_type, status,
             command, rendered_diff, citations, domain_path, domain_method, domain_body)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7, $8, $9, $10, $11, $12)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(user_id)
    .bind(interaction_id)
    .bind(&draft.tool_name)
    .bind(&draft.action_type)
    .bind(&draft.command)
    .bind(&draft.rendered_diff)
    .bind(citations_json)
    .bind(&draft.domain_path)
    .bind(&draft.domain_method)
    .bind(&draft.domain_body)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(id)
}
