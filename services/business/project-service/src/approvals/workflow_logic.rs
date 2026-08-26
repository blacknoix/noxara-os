//! Pure ApprovalProcess state machine — used by Temporal workflow and unit tests.
//!
//! Timers live in Temporal; this module models decide / escalate transitions so
//! tests can prove duplicate decide is a no-op and SLA escalation advances
//! without depending on a live Temporal cluster.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessStatus {
    Pending,
    Approved,
    Rejected,
    Escalated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessState {
    pub status: ProcessStatus,
    pub current_step: i32,
    pub max_step: i32,
    pub mode: String,
    pub decided: bool,
    pub escalations: u32,
}

impl ProcessState {
    pub fn new(current_step: i32, max_step: i32, mode: impl Into<String>) -> Self {
        Self {
            status: ProcessStatus::Pending,
            current_step,
            max_step,
            mode: mode.into(),
            decided: false,
            escalations: 0,
        }
    }

    /// Apply a decide signal. Duplicate decides are no-ops.
    pub fn apply_decide(&mut self, approve: bool) -> bool {
        if self.decided || self.status != ProcessStatus::Pending {
            return false;
        }
        self.decided = true;
        self.status = if approve {
            ProcessStatus::Approved
        } else {
            ProcessStatus::Rejected
        };
        true
    }

    /// SLA timer fired: escalate to next step/role. No-op if already decided.
    /// Returns true when an escalation was applied.
    pub fn apply_sla_timeout(&mut self) -> bool {
        if self.decided || self.status != ProcessStatus::Pending {
            return false;
        }
        self.escalations += 1;
        if self.current_step < self.max_step {
            self.current_step += 1;
            self.status = ProcessStatus::Pending;
        } else {
            self.status = ProcessStatus::Escalated;
        }
        true
    }

    /// Workflow ID format: `{org_id}:ApprovalProcess:{approval_id}`.
    pub fn workflow_id(org_id: &str, approval_id: &str) -> String {
        format!("{org_id}:ApprovalProcess:{approval_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_decide_is_noop() {
        let mut s = ProcessState::new(1, 2, "sequential");
        assert!(s.apply_decide(true));
        assert!(!s.apply_decide(false));
        assert_eq!(s.status, ProcessStatus::Approved);
    }

    #[test]
    fn sla_escalates_then_terminal() {
        let mut s = ProcessState::new(1, 2, "sequential");
        assert!(s.apply_sla_timeout());
        assert_eq!(s.current_step, 2);
        assert_eq!(s.status, ProcessStatus::Pending);
        assert!(s.apply_sla_timeout());
        assert_eq!(s.status, ProcessStatus::Escalated);
        // Timer after terminal is a no-op.
        assert!(!s.apply_sla_timeout());
    }

    #[test]
    fn timer_survives_restart_semantics() {
        // Simulate worker restart: serialize state, restore, fire timer.
        let s = ProcessState::new(1, 3, "sequential");
        let json = serde_json::to_string(&s).unwrap();
        let mut restored: ProcessState = serde_json::from_str(&json).unwrap();
        assert!(restored.apply_sla_timeout());
        assert_eq!(restored.current_step, 2);
        assert_eq!(restored.escalations, 1);
        // Decide after restore still works once.
        assert!(restored.apply_decide(true));
        assert!(!restored.apply_decide(true));
    }

    #[test]
    fn workflow_id_format() {
        assert_eq!(
            ProcessState::workflow_id("org_abc", "apr_xyz"),
            "org_abc:ApprovalProcess:apr_xyz"
        );
    }
}
