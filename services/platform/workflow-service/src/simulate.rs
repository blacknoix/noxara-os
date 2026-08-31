//! Pure simulation / dry-run — **no side effects**.
//!
//! Does not write to the database, does not insert outbox rows, and does not
//! perform HTTP mutations. Used by `/simulate` and unit tests.

use companyos_authz::Principal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::definition::{HumanStepKind, WorkflowGraph, WorkflowNode};
use crate::permissions::enforce_action_permission;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SimulateStepResult {
    pub step_index: i32,
    pub node_id: String,
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_allowed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SimulateResult {
    pub ok: bool,
    pub steps: Vec<SimulateStepResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Always true — documents the dry-run contract for clients/tests.
    pub side_effects: bool,
}

fn json_path<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for part in path.split('.').filter(|p| !p.is_empty()) {
        cur = cur.get(part)?;
    }
    Some(cur)
}

fn render_templates(value: &serde_json::Value, payload: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            let mut out = s.clone();
            if let Some(obj) = payload.as_object() {
                for (k, v) in obj {
                    let needle = format!("{{{{payload.{k}}}}}");
                    let repl = match v {
                        serde_json::Value::String(x) => x.clone(),
                        other => other.to_string(),
                    };
                    out = out.replace(&needle, &repl);
                }
            }
            serde_json::Value::String(out)
        }
        serde_json::Value::Object(map) => {
            let mut n = serde_json::Map::new();
            for (k, v) in map {
                n.insert(k.clone(), render_templates(v, payload));
            }
            serde_json::Value::Object(n)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| render_templates(v, payload)).collect())
        }
        other => other.clone(),
    }
}

/// Walk the graph against a sample payload. Caps steps the same as production.
pub fn simulate(
    graph: &WorkflowGraph,
    principal: &Principal,
    payload: &serde_json::Value,
    max_steps: i32,
) -> SimulateResult {
    let mut steps = Vec::new();
    let mut current = Some(graph.entry.clone());
    let mut step_index = 0i32;

    while let Some(node_id) = current {
        if step_index >= max_steps {
            return SimulateResult {
                ok: false,
                steps,
                error: Some(format!(
                    "iteration/step cap exceeded ({max_steps}); run failed closed"
                )),
                side_effects: false,
            };
        }

        let Some(node) = graph.node(&node_id) else {
            return SimulateResult {
                ok: false,
                steps,
                error: Some(format!("unknown node '{node_id}'")),
                side_effects: false,
            };
        };

        match node {
            WorkflowNode::End { id } => {
                steps.push(SimulateStepResult {
                    step_index,
                    node_id: id.clone(),
                    node_type: "end".into(),
                    action: None,
                    status: "ok".into(),
                    detail: Some("completed".into()),
                    permission: None,
                    permission_allowed: None,
                });
                return SimulateResult {
                    ok: true,
                    steps,
                    error: None,
                    side_effects: false,
                };
            }
            WorkflowNode::Action {
                id,
                action,
                params,
                next,
            } => {
                let perm_check = enforce_action_permission(principal, action, "simulate");
                let (status, detail, perm, allowed) = match &perm_check {
                    Ok(pid) => (
                        "ok".to_string(),
                        Some(format!(
                            "would call action '{action}' with {}",
                            render_templates(params, payload)
                        )),
                        Some(pid.as_str().to_string()),
                        Some(true),
                    ),
                    Err(e) => (
                        "denied".to_string(),
                        Some(e.detail.clone()),
                        required_perm_str(action),
                        Some(false),
                    ),
                };
                steps.push(SimulateStepResult {
                    step_index,
                    node_id: id.clone(),
                    node_type: "action".into(),
                    action: Some(action.clone()),
                    status: status.clone(),
                    detail,
                    permission: perm,
                    permission_allowed: allowed,
                });
                if status == "denied" {
                    return SimulateResult {
                        ok: false,
                        steps,
                        error: Some(format!("action '{action}' denied")),
                        side_effects: false,
                    };
                }
                current = next.clone();
            }
            WorkflowNode::Condition {
                id,
                path,
                equals,
                then_next,
                else_next,
            } => {
                let actual = json_path(payload, path.strip_prefix("payload.").unwrap_or(path));
                let matched = actual == Some(equals);
                steps.push(SimulateStepResult {
                    step_index,
                    node_id: id.clone(),
                    node_type: "condition".into(),
                    action: None,
                    status: "ok".into(),
                    detail: Some(format!("matched={matched}")),
                    permission: None,
                    permission_allowed: None,
                });
                current = Some(if matched {
                    then_next.clone()
                } else {
                    else_next.clone()
                });
            }
            WorkflowNode::Branch {
                id,
                arms,
                default_next,
            } => {
                let mut chosen = default_next.clone();
                for arm in arms {
                    let path = arm.path.strip_prefix("payload.").unwrap_or(&arm.path);
                    if json_path(payload, path) == Some(&arm.equals) {
                        chosen = arm.next.clone();
                        break;
                    }
                }
                steps.push(SimulateStepResult {
                    step_index,
                    node_id: id.clone(),
                    node_type: "branch".into(),
                    action: None,
                    status: "ok".into(),
                    detail: Some(format!("next={chosen}")),
                    permission: None,
                    permission_allowed: None,
                });
                current = Some(chosen);
            }
            WorkflowNode::Timer {
                id,
                duration_secs,
                next,
            } => {
                steps.push(SimulateStepResult {
                    step_index,
                    node_id: id.clone(),
                    node_type: "timer".into(),
                    action: None,
                    status: "waiting".into(),
                    detail: Some(format!("would wait {duration_secs}s (no timer started)")),
                    permission: None,
                    permission_allowed: None,
                });
                current = Some(next.clone());
            }
            WorkflowNode::Human {
                id,
                kind,
                params: _,
                on_approve,
                on_reject: _,
            } => {
                let action_key = match kind {
                    HumanStepKind::Approval => Some("start_approval"),
                    HumanStepKind::Inbox => None,
                };
                if let Some(ak) = action_key {
                    if let Err(e) = enforce_action_permission(principal, ak, "simulate") {
                        steps.push(SimulateStepResult {
                            step_index,
                            node_id: id.clone(),
                            node_type: "human".into(),
                            action: Some(ak.into()),
                            status: "denied".into(),
                            detail: Some(e.detail),
                            permission: required_perm_str(ak),
                            permission_allowed: Some(false),
                        });
                        return SimulateResult {
                            ok: false,
                            steps,
                            error: Some("human/approval step denied".into()),
                            side_effects: false,
                        };
                    }
                }
                steps.push(SimulateStepResult {
                    step_index,
                    node_id: id.clone(),
                    node_type: "human".into(),
                    action: action_key.map(str::to_string),
                    status: "waiting".into(),
                    detail: Some(
                        "would wait for human decision (simulation assumes approve)".into(),
                    ),
                    permission: action_key.and_then(required_perm_str),
                    permission_allowed: Some(true),
                });
                current = Some(on_approve.clone());
            }
        }
        step_index += 1;
    }

    SimulateResult {
        ok: false,
        steps,
        error: Some("graph ended without an End node".into()),
        side_effects: false,
    }
}

fn required_perm_str(action: &str) -> Option<String> {
    crate::catalogue::action_entry(action).map(|a| a.required_permission.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{WorkflowGraph, WorkflowNode, WorkflowTrigger};
    use companyos_authz::Role;

    #[test]
    fn dry_run_has_no_side_effects_flag() {
        let g = WorkflowGraph {
            entry: "a".into(),
            trigger: WorkflowTrigger::Manual,
            nodes: vec![
                WorkflowNode::Action {
                    id: "a".into(),
                    action: "create_task".into(),
                    params: serde_json::json!({"title": "hi"}),
                    next: Some("e".into()),
                },
                WorkflowNode::End { id: "e".into() },
            ],
            sla_seconds: None,
        };
        let p = Principal::with_roles(vec![Role::Member]);
        let r = simulate(&g, &p, &serde_json::json!({}), 100);
        assert!(r.ok);
        assert!(!r.side_effects);
    }

    #[test]
    fn dry_run_denies_payroll_for_member() {
        let g = WorkflowGraph {
            entry: "a".into(),
            trigger: WorkflowTrigger::Manual,
            nodes: vec![
                WorkflowNode::Action {
                    id: "a".into(),
                    action: "read_payroll".into(),
                    params: serde_json::json!({}),
                    next: Some("e".into()),
                },
                WorkflowNode::End { id: "e".into() },
            ],
            sla_seconds: None,
        };
        let p = Principal::with_roles(vec![Role::Member]);
        let r = simulate(&g, &p, &serde_json::json!({}), 100);
        assert!(!r.ok);
        assert_eq!(r.steps[0].status, "denied");
        assert!(!r.side_effects);
    }

    #[test]
    fn step_cap_fails_closed() {
        let g = WorkflowGraph {
            entry: "loop".into(),
            trigger: WorkflowTrigger::Manual,
            nodes: vec![WorkflowNode::Action {
                id: "loop".into(),
                action: "create_task".into(),
                params: serde_json::json!({}),
                next: Some("loop".into()),
            }],
            sla_seconds: None,
        };
        let p = Principal::with_roles(vec![Role::Member]);
        let r = simulate(&g, &p, &serde_json::json!({}), 5);
        assert!(!r.ok);
        assert!(r.error.as_ref().unwrap().contains("cap exceeded"));
        assert!(!r.side_effects);
    }
}
