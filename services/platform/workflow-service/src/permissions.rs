//! Permission gating: a workflow cannot exceed its creator's permissions.
//!
//! Checked at save/publish time and again at every action step at run time.
//! Deny by default for unknown actions.

use companyos_authz::{decide, Decision, PermissionId, Principal};
use companyos_errors::{AppError, ErrorCode};

use crate::catalogue::{required_permission_for_action, required_permissions_for_graph};
use crate::definition::WorkflowGraph;

/// Ensure the principal is allowed every permission the graph requires.
pub fn enforce_creator_can_own_graph(
    principal: &Principal,
    graph: &WorkflowGraph,
    request_id: &str,
) -> Result<Vec<String>, AppError> {
    let required = required_permissions_for_graph(graph)
        .map_err(|e| AppError::new(ErrorCode::ValidationFailed, request_id, e))?;
    for perm in &required {
        let pid = PermissionId::from(perm.as_str());
        let d = decide(principal, &pid);
        if d.decision != Decision::Allow {
            return Err(AppError::new(
                ErrorCode::Forbidden,
                request_id,
                format!(
                    "workflow creator lacks permission '{perm}' required by an action in this definition (deny by default)"
                ),
            ));
        }
    }
    Ok(required)
}

/// Runtime check before executing a single action (same principal as creator / recorded actor).
pub fn enforce_action_permission(
    principal: &Principal,
    action_key: &str,
    request_id: &str,
) -> Result<PermissionId, AppError> {
    let Some(pid) = required_permission_for_action(action_key) else {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!("unknown action '{action_key}' — deny by default"),
        ));
    };
    let d = decide(principal, &pid);
    if d.decision != Decision::Allow {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!(
                "permission '{}' denied for action '{action_key}' at run time",
                pid.as_str()
            ),
        ));
    }
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{WorkflowGraph, WorkflowNode, WorkflowTrigger};
    use companyos_authz::Role;

    fn graph_with_action(action: &str) -> WorkflowGraph {
        WorkflowGraph {
            entry: "a".into(),
            trigger: WorkflowTrigger::Manual,
            nodes: vec![
                WorkflowNode::Action {
                    id: "a".into(),
                    action: action.into(),
                    params: serde_json::json!({}),
                    next: Some("e".into()),
                },
                WorkflowNode::End { id: "e".into() },
            ],
            sla_seconds: None,
        }
    }

    #[test]
    fn member_cannot_own_payroll_or_journal_actions() {
        let member = Principal::with_roles(vec![Role::Member]);
        let payroll = graph_with_action("read_payroll");
        let journal = graph_with_action("post_journal");
        assert!(enforce_creator_can_own_graph(&member, &payroll, "t").is_err());
        assert!(enforce_creator_can_own_graph(&member, &journal, "t").is_err());
        let task = graph_with_action("create_task");
        assert!(enforce_creator_can_own_graph(&member, &task, "t").is_ok());
    }

    #[test]
    fn owner_can_own_high_risk() {
        let owner = Principal::with_roles(vec![Role::Owner]);
        let journal = graph_with_action("post_journal");
        assert!(enforce_creator_can_own_graph(&owner, &journal, "t").is_ok());
    }
}
