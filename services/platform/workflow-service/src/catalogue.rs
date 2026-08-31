//! Trigger + action catalogues — configuration keys only (no free-form code).
//!
//! Triggers map to domain events already emitted by business services.
//! Actions map to existing service APIs and required authz permissions.

use companyos_authz::PermissionId;
use serde::Serialize;
use utoipa::ToSchema;

/// One trigger a definition may bind to.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TriggerCatalogueEntry {
    pub event_key: &'static str,
    pub context: &'static str,
    pub aggregate: &'static str,
    pub event_type: &'static str,
    pub description: &'static str,
    /// Subject suffix after org: `{context}.{aggregate}.{event}.v1`
    pub subject_suffix: &'static str,
}

/// One action a step may invoke.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ActionCatalogueEntry {
    pub key: &'static str,
    pub description: &'static str,
    /// Permission checked at save time and at run time (deny by default).
    pub required_permission: &'static str,
    /// Relative HTTP path under the owning service (for documentation / activities).
    pub http_method: &'static str,
    pub http_path: &'static str,
    /// High-risk actions (journals) stay in catalogue but Member cannot use them.
    pub high_risk: bool,
}

pub const TRIGGER_CATALOGUE: &[TriggerCatalogueEntry] = &[
    TriggerCatalogueEntry {
        event_key: "sales.deal.won",
        context: "sales",
        aggregate: "deal",
        event_type: "won",
        description: "Deal marked won in CRM",
        subject_suffix: "sales.deal.won.v1",
    },
    TriggerCatalogueEntry {
        event_key: "people.leave.requested",
        context: "people",
        aggregate: "leave",
        event_type: "requested",
        description: "Leave request submitted",
        subject_suffix: "people.leave.requested.v1",
    },
    TriggerCatalogueEntry {
        event_key: "people.leave.approved",
        context: "people",
        aggregate: "leave",
        event_type: "approved",
        description: "Leave request approved",
        subject_suffix: "people.leave.approved.v1",
    },
    TriggerCatalogueEntry {
        event_key: "inventory.stock.low",
        context: "inventory",
        aggregate: "stock",
        event_type: "low",
        description: "Stock at or below reorder point",
        subject_suffix: "inventory.stock.low.v1",
    },
    TriggerCatalogueEntry {
        event_key: "inventory.purchase_request.approved",
        context: "inventory",
        aggregate: "purchase_request",
        event_type: "approved",
        description: "Purchase request approved",
        subject_suffix: "inventory.purchase_request.approved.v1",
    },
    TriggerCatalogueEntry {
        event_key: "finance.invoice.issued",
        context: "finance",
        aggregate: "invoice",
        event_type: "issued",
        description: "Invoice issued (overdue is query-status; use issued as trigger)",
        subject_suffix: "finance.invoice.issued.v1",
    },
];

pub const ACTION_CATALOGUE: &[ActionCatalogueEntry] = &[
    ActionCatalogueEntry {
        key: "create_task",
        description: "Create an operations task",
        required_permission: "operations.task.create",
        http_method: "POST",
        http_path: "/api/v1/operations/tasks",
        high_risk: false,
    },
    ActionCatalogueEntry {
        key: "send_notification",
        description: "Send an in-app notification via notification ingest",
        required_permission: "platform.notification.read",
        http_method: "POST",
        http_path: "/api/v1/notifications/internal/ingest",
        high_risk: false,
    },
    ActionCatalogueEntry {
        key: "start_approval",
        description: "Start an approval process (operations approvals API)",
        required_permission: "operations.approval.read",
        http_method: "POST",
        http_path: "/api/v1/operations/approvals",
        high_risk: false,
    },
    ActionCatalogueEntry {
        key: "update_deal_status",
        description: "Update a CRM deal (status / fields)",
        required_permission: "sales.deal.update",
        http_method: "PATCH",
        http_path: "/api/v1/sales/deals/{id}",
        high_risk: false,
    },
    ActionCatalogueEntry {
        key: "create_purchase_request",
        description: "Draft a purchase request (low-stock → PR)",
        required_permission: "inventory.purchase_request.write",
        http_method: "POST",
        http_path: "/api/v1/inventory/purchase-requests",
        high_risk: false,
    },
    ActionCatalogueEntry {
        key: "post_journal",
        description: "Post a journal entry (high-risk; integer minor units; closed periods reject)",
        required_permission: "finance.journal.post",
        http_method: "POST",
        http_path: "/api/v1/finance/journals",
        high_risk: true,
    },
    ActionCatalogueEntry {
        key: "read_payroll",
        description: "Read payroll run (sensitive; Member must not gain this via workflow)",
        required_permission: "hr.payroll.read",
        http_method: "GET",
        http_path: "/api/v1/people/payroll/runs",
        high_risk: true,
    },
];

pub fn is_known_trigger(event_key: &str) -> bool {
    TRIGGER_CATALOGUE.iter().any(|t| t.event_key == event_key)
}

pub fn is_known_action(key: &str) -> bool {
    ACTION_CATALOGUE.iter().any(|a| a.key == key)
}

pub fn action_entry(key: &str) -> Option<&'static ActionCatalogueEntry> {
    ACTION_CATALOGUE.iter().find(|a| a.key == key)
}

pub fn trigger_entry(event_key: &str) -> Option<&'static TriggerCatalogueEntry> {
    TRIGGER_CATALOGUE.iter().find(|t| t.event_key == event_key)
}

pub fn required_permission_for_action(key: &str) -> Option<PermissionId> {
    action_entry(key).map(|a| PermissionId::from(a.required_permission))
}

/// Resolve permissions required by a graph (deny-by-default unknown actions).
pub fn required_permissions_for_graph(
    graph: &crate::definition::WorkflowGraph,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for key in graph.action_keys() {
        let Some(entry) = action_entry(&key) else {
            return Err(format!("unknown action '{key}'"));
        };
        out.push(entry.required_permission.to_string());
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Fixture graphs shipped as examples (not five customer workshops).
pub fn fixture_graphs() -> Vec<(&'static str, &'static str, crate::definition::WorkflowGraph)> {
    use crate::definition::{WorkflowGraph, WorkflowNode, WorkflowTrigger};

    let deal_won = WorkflowGraph {
        entry: "create_followup".into(),
        trigger: WorkflowTrigger::DomainEvent {
            event_key: "sales.deal.won".into(),
        },
        nodes: vec![
            WorkflowNode::Action {
                id: "create_followup".into(),
                action: "create_task".into(),
                params: serde_json::json!({
                    "title": "Follow up on won deal",
                    "description": "Deal {{payload.deal_id}} was won — schedule kickoff."
                }),
                next: Some("done".into()),
            },
            WorkflowNode::End { id: "done".into() },
        ],
        sla_seconds: Some(86_400),
    };

    let leave_approved = WorkflowGraph {
        entry: "notify".into(),
        trigger: WorkflowTrigger::DomainEvent {
            event_key: "people.leave.approved".into(),
        },
        nodes: vec![
            WorkflowNode::Action {
                id: "notify".into(),
                action: "send_notification".into(),
                params: serde_json::json!({
                    "title": "Leave approved",
                    "body": "Leave request {{payload.id}} was approved."
                }),
                next: Some("done".into()),
            },
            WorkflowNode::End { id: "done".into() },
        ],
        sla_seconds: None,
    };

    let low_stock = WorkflowGraph {
        entry: "draft_pr".into(),
        trigger: WorkflowTrigger::DomainEvent {
            event_key: "inventory.stock.low".into(),
        },
        nodes: vec![
            WorkflowNode::Action {
                id: "draft_pr".into(),
                action: "create_purchase_request".into(),
                params: serde_json::json!({
                    "notes": "Auto-draft from low stock on {{payload.item_id}}"
                }),
                next: Some("done".into()),
            },
            WorkflowNode::End { id: "done".into() },
        ],
        sla_seconds: Some(172_800),
    };

    vec![
        (
            "Deal won → follow-up task",
            "Creates an ops task when a deal is won.",
            deal_won,
        ),
        (
            "Leave approved → notify",
            "Sends an in-app notification when leave is approved.",
            leave_approved,
        ),
        (
            "Low stock → purchase request draft",
            "Drafts a purchase request when stock is low.",
            low_stock,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_validate() {
        for (name, _, g) in fixture_graphs() {
            g.validate()
                .unwrap_or_else(|e| panic!("fixture {name} invalid: {e}"));
            required_permissions_for_graph(&g).unwrap();
        }
    }

    #[test]
    fn high_risk_actions_present() {
        assert!(action_entry("post_journal").unwrap().high_risk);
        assert!(action_entry("read_payroll").unwrap().high_risk);
        assert!(!action_entry("create_task").unwrap().high_risk);
    }
}
