//! EmployeeOffboarding — Temporal workflow state machine (Phase 2.1).
//!
//! Revokes every access path this system owns (membership, sessions, API keys
//! N/A, integration tokens N/A). Workflow id:
//! `{org_id}:EmployeeOffboarding:{employee_id}`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Started,
    MarkedOffboarding,
    ReportsReassigned,
    ReturnTasksCreated,
    AccessRevoked,
    Terminated,
    Notified,
    Completed,
    Compensating,
    Compensated,
    Failed,
    BlockedLastOwner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub employee_id: String,
    pub org_id: String,
    pub steps_done: Vec<String>,
    pub checklist: Vec<ChecklistItem>,
    pub fail_after: Option<String>,
    pub compensation_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChecklistItem {
    pub path: String,
    pub cleared: bool,
    pub detail: String,
}

impl State {
    pub fn start(org_id: impl Into<String>, employee_id: impl Into<String>) -> Self {
        Self {
            status: Status::Started,
            employee_id: employee_id.into(),
            org_id: org_id.into(),
            steps_done: Vec::new(),
            checklist: Vec::new(),
            fail_after: None,
            compensation_reason: None,
        }
    }

    pub fn with_fail_after(mut self, step: Option<String>) -> Self {
        self.fail_after = step;
        self
    }

    fn maybe_fail(&mut self, just_completed: &str) -> bool {
        if self.fail_after.as_deref() == Some(just_completed) {
            self.status = Status::Compensating;
            self.compensation_reason = Some(format!("injected failure after {just_completed}"));
            true
        } else {
            false
        }
    }

    pub fn signal_advance(&mut self, step: &str) -> bool {
        if matches!(
            self.status,
            Status::Completed
                | Status::Compensated
                | Status::Failed
                | Status::BlockedLastOwner
        ) {
            return false;
        }

        let next = match (self.status, step) {
            (Status::Started, "mark_offboarding") => Status::MarkedOffboarding,
            (Status::MarkedOffboarding, "reassign_reports") => Status::ReportsReassigned,
            (Status::ReportsReassigned, "create_return_tasks") => Status::ReturnTasksCreated,
            (Status::ReturnTasksCreated, "revoke_access") => Status::AccessRevoked,
            (Status::AccessRevoked, "terminate") => Status::Terminated,
            (Status::Terminated, "notify") => Status::Notified,
            (Status::Notified, "complete") => Status::Completed,
            _ => return false,
        };
        self.status = next;
        self.steps_done.push(step.to_string());
        if self.maybe_fail(step) {
            return true;
        }
        true
    }

    pub fn signal_access_checklist(&mut self, items: Vec<ChecklistItem>) {
        self.checklist = items;
    }

    pub fn signal_blocked_last_owner(&mut self) {
        self.status = Status::BlockedLastOwner;
        self.compensation_reason = Some("last active Owner".into());
    }

    pub fn signal_compensate(&mut self) -> bool {
        if matches!(self.status, Status::Completed | Status::Compensated | Status::BlockedLastOwner)
        {
            return false;
        }
        self.status = Status::Compensated;
        true
    }

    pub fn all_access_cleared(&self) -> bool {
        !self.checklist.is_empty() && self.checklist.iter().all(|c| c.cleared)
    }

    pub fn query(&self) -> &Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_with_checklist() {
        let mut s = State::start("org_a", "emp_b");
        for step in [
            "mark_offboarding",
            "reassign_reports",
            "create_return_tasks",
            "revoke_access",
            "terminate",
            "notify",
            "complete",
        ] {
            assert!(s.signal_advance(step), "failed at {step}");
        }
        s.signal_access_checklist(vec![
            ChecklistItem {
                path: "membership".into(),
                cleared: true,
                detail: "ok".into(),
            },
            ChecklistItem {
                path: "sessions".into(),
                cleared: true,
                detail: "ok".into(),
            },
            ChecklistItem {
                path: "api_keys".into(),
                cleared: true,
                detail: "n/a".into(),
            },
            ChecklistItem {
                path: "integration_tokens".into(),
                cleared: true,
                detail: "n/a".into(),
            },
        ]);
        assert_eq!(s.status, Status::Completed);
        assert!(s.all_access_cleared());
    }

    #[test]
    fn last_owner_blocks() {
        let mut s = State::start("org_a", "emp_b");
        assert!(s.signal_advance("mark_offboarding"));
        s.signal_blocked_last_owner();
        assert_eq!(s.status, Status::BlockedLastOwner);
    }
}
