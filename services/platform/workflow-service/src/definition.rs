//! Workflow graph model — configuration, not free-form code.
//!
//! A definition is a directed graph of typed nodes (trigger entry + steps).
//! Publishing snapshots an immutable version; instances pin that version.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Top-level workflow graph stored on a version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct WorkflowGraph {
    /// Entry node id after the trigger fires.
    pub entry: String,
    pub trigger: WorkflowTrigger,
    pub nodes: Vec<WorkflowNode>,
    /// Optional SLA deadline from start (seconds). Soft signal for monitor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sla_seconds: Option<u64>,
}

/// Domain-event trigger (from existing emitted events).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowTrigger {
    /// Match a catalogue event key, e.g. `sales.deal.won`.
    DomainEvent { event_key: String },
    /// Manual / API start only.
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowNode {
    Action {
        id: String,
        /// Catalogue action key (create_task, send_notification, …).
        action: String,
        #[serde(default)]
        params: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    Condition {
        id: String,
        /// Simple path equality: `payload.field == value`.
        path: String,
        #[serde(default)]
        equals: serde_json::Value,
        then_next: String,
        else_next: String,
    },
    Branch {
        id: String,
        /// First matching arm wins; else `default_next`.
        arms: Vec<BranchArm>,
        default_next: String,
    },
    Timer {
        id: String,
        duration_secs: u64,
        next: String,
    },
    Human {
        id: String,
        /// `approval` → start approval via operations API; `inbox` → wait signal.
        kind: HumanStepKind,
        #[serde(default)]
        params: serde_json::Value,
        on_approve: String,
        on_reject: String,
    },
    End {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct BranchArm {
    pub path: String,
    pub equals: serde_json::Value,
    pub next: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HumanStepKind {
    Approval,
    Inbox,
}

impl WorkflowNode {
    pub fn id(&self) -> &str {
        match self {
            Self::Action { id, .. }
            | Self::Condition { id, .. }
            | Self::Branch { id, .. }
            | Self::Timer { id, .. }
            | Self::Human { id, .. }
            | Self::End { id } => id,
        }
    }

    pub fn as_action_key(&self) -> Option<&str> {
        match self {
            Self::Action { action, .. } => Some(action.as_str()),
            Self::Human {
                kind: HumanStepKind::Approval,
                ..
            } => Some("start_approval"),
            _ => None,
        }
    }
}

impl WorkflowGraph {
    pub fn node(&self, id: &str) -> Option<&WorkflowNode> {
        self.nodes.iter().find(|n| n.id() == id)
    }

    /// Collect action keys that require permission checks.
    pub fn action_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        for n in &self.nodes {
            if let Some(k) = n.as_action_key() {
                keys.push(k.to_string());
            }
        }
        keys.sort();
        keys.dedup();
        keys
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.entry.trim().is_empty() {
            return Err("entry node id is required".into());
        }
        if self.node(&self.entry).is_none() {
            return Err(format!("entry node '{}' not found in nodes", self.entry));
        }
        let mut seen = std::collections::HashSet::new();
        for n in &self.nodes {
            if !seen.insert(n.id().to_string()) {
                return Err(format!("duplicate node id '{}'", n.id()));
            }
        }
        match &self.trigger {
            WorkflowTrigger::DomainEvent { event_key } => {
                if !crate::catalogue::is_known_trigger(event_key) {
                    return Err(format!("unknown trigger event_key '{event_key}'"));
                }
            }
            WorkflowTrigger::Manual => {}
        }
        for n in &self.nodes {
            if let WorkflowNode::Action { action, .. } = n {
                if !crate::catalogue::is_known_action(action) {
                    return Err(format!("unknown action '{action}'"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_fixture_deal_won() {
        let g = serde_json::from_value::<WorkflowGraph>(serde_json::json!({
            "entry": "create_followup",
            "trigger": { "kind": "domain_event", "event_key": "sales.deal.won" },
            "nodes": [
                {
                    "type": "action",
                    "id": "create_followup",
                    "action": "create_task",
                    "params": { "title": "Follow up on won deal {{payload.deal_id}}" },
                    "next": "done"
                },
                { "type": "end", "id": "done" }
            ]
        }))
        .unwrap();
        assert!(g.validate().is_ok());
        assert_eq!(g.action_keys(), vec!["create_task".to_string()]);
    }
}
