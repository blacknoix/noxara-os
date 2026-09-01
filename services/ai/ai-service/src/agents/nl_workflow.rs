//! Natural-language → 3.1 workflow definition proposal.
//!
//! Human publishes via the existing workflow builder. NL cannot emit free-form
//! code or skip catalogue / permission gates.

use companyos_authz::{PermissionId, Principal};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::{set_session_org_id, OrgId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::provider::wrap_untrusted;
use crate::state::AppState;

/// Catalogue mirrors workflow-service (keep in sync; NL cannot invent keys).
const TRIGGERS: &[&str] = &[
    "sales.deal.won",
    "people.leave.requested",
    "people.leave.approved",
    "inventory.stock.low",
    "inventory.purchase_request.approved",
    "finance.invoice.issued",
];

struct ActionCat {
    key: &'static str,
    permission: &'static str,
    high_risk: bool,
}

const ACTIONS: &[ActionCat] = &[
    ActionCat {
        key: "create_task",
        permission: "operations.task.create",
        high_risk: false,
    },
    ActionCat {
        key: "send_notification",
        permission: "platform.notification.read",
        high_risk: false,
    },
    ActionCat {
        key: "start_approval",
        permission: "operations.approval.read",
        high_risk: false,
    },
    ActionCat {
        key: "update_deal_status",
        permission: "sales.deal.update",
        high_risk: false,
    },
    ActionCat {
        key: "create_purchase_request",
        permission: "inventory.purchase_request.write",
        high_risk: false,
    },
    ActionCat {
        key: "post_journal",
        permission: "finance.journal.post",
        high_risk: true,
    },
    ActionCat {
        key: "read_payroll",
        permission: "hr.payroll.read",
        high_risk: true,
    },
];

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NlWorkflowRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NlWorkflowDraft {
    pub id: String,
    pub status: String,
    pub prompt: String,
    pub definition: Value,
    pub filtered_actions: Vec<String>,
    pub note: String,
}

/// Parse NL into a draft definition using only catalogue keys the author may use.
pub async fn propose_workflow(
    state: &AppState,
    org_id: OrgId,
    user_id: Uuid,
    principal: &Principal,
    req: &NlWorkflowRequest,
    request_id: &str,
) -> Result<NlWorkflowDraft, AppError> {
    // Untrusted NL is data — wrap before any model involvement.
    let _wrapped = wrap_untrusted(&req.prompt);
    let lower = req.prompt.to_ascii_lowercase();

    let trigger = detect_trigger(&lower).unwrap_or("manual");
    let mut wanted = detect_actions(&lower);
    if wanted.is_empty() {
        wanted.push("create_task");
        wanted.push("send_notification");
    }

    let mut steps = Vec::new();
    let mut filtered = Vec::new();
    for key in &wanted {
        let Some(cat) = ACTIONS.iter().find(|a| a.key == *key) else {
            continue;
        };
        if cat.high_risk
            && !companyos_authz::is_allowed(principal, &PermissionId::from(cat.permission))
        {
            filtered.push(format!("{} (denied: lacks {})", cat.key, cat.permission));
            continue;
        }
        if !companyos_authz::is_allowed(principal, &PermissionId::from(cat.permission)) {
            filtered.push(format!("{} (denied: lacks {})", cat.key, cat.permission));
            continue;
        }
        steps.push(json!({
            "action": cat.key,
            "required_permission": cat.permission,
            "config": {}
        }));
    }

    // Fail closed: never include actions the author cannot perform.
    let definition = json!({
        "name": "NL draft workflow",
        "description": format!("Proposed from natural language (human must publish). Trigger hint: {trigger}"),
        "status": "draft",
        "trigger": { "type": if trigger == "manual" { "manual" } else { "event" }, "event_key": trigger },
        "steps": steps,
        "source": "ai.nl_workflow.v1",
    });

    let id = new_uuid_v7();
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
        INSERT INTO ai_workflow_draft (id, org_id, user_id, prompt, definition, filtered_actions, status)
        VALUES ($1,$2,$3,$4,$5,$6,'draft')
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(user_id)
    .bind(&req.prompt)
    .bind(&definition)
    .bind(json!(filtered))
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(NlWorkflowDraft {
        id: id.to_string(),
        status: "draft".into(),
        prompt: req.prompt.clone(),
        definition,
        filtered_actions: filtered,
        note:
            "Draft only — publish via /workflows (operations.workflow.publish). AI cannot publish."
                .into(),
    })
}

fn detect_trigger(lower: &str) -> Option<&'static str> {
    for t in TRIGGERS {
        let needle = t.replace('.', " ");
        if lower.contains(t) || lower.contains(&needle) {
            return Some(*t);
        }
        if *t == "sales.deal.won" && (lower.contains("deal won") || lower.contains("when a deal")) {
            return Some(*t);
        }
        if *t == "finance.invoice.issued"
            && (lower.contains("invoice issued") || lower.contains("when an invoice"))
        {
            return Some(*t);
        }
        if *t == "inventory.stock.low" && lower.contains("low stock") {
            return Some(*t);
        }
        if *t == "people.leave.approved" && lower.contains("leave approved") {
            return Some(*t);
        }
    }
    None
}

fn detect_actions(lower: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if lower.contains("task") || lower.contains("create a task") {
        out.push("create_task");
    }
    if lower.contains("notif") || lower.contains("notify") || lower.contains("alert") {
        out.push("send_notification");
    }
    if lower.contains("approval") {
        out.push("start_approval");
    }
    if lower.contains("deal status") || lower.contains("update deal") {
        out.push("update_deal_status");
    }
    if lower.contains("purchase request") || lower.contains("reorder") {
        out.push("create_purchase_request");
    }
    // High-risk: only if explicitly requested — still filtered by perms.
    if lower.contains("post journal") || lower.contains("journal entry") {
        out.push("post_journal");
    }
    if lower.contains("payroll") {
        out.push("read_payroll");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_authz::Role;

    #[test]
    fn member_cannot_get_payroll_or_journal_in_draft() {
        let member = Principal::with_roles(vec![Role::Member]);
        let lower = "when deal won post journal and read payroll and create task";
        let mut wanted = detect_actions(lower);
        assert!(wanted.contains(&"post_journal"));
        assert!(wanted.contains(&"read_payroll"));
        wanted.retain(|k| {
            let cat = ACTIONS.iter().find(|a| a.key == *k).unwrap();
            companyos_authz::is_allowed(&member, &PermissionId::from(cat.permission))
        });
        assert!(wanted.contains(&"create_task"));
        assert!(!wanted.contains(&"post_journal"));
        assert!(!wanted.contains(&"read_payroll"));
    }
}
