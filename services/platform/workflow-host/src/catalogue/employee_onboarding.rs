//! EmployeeOnboarding — Temporal workflow state machine (Phase 2.1).
//!
//! Activities call People APIs with `on_behalf_of`. Workflow id:
//! `{org_id}:EmployeeOnboarding:{employee_id}` (idempotent).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Started,
    UserLinked,
    RoleAssigned,
    AssetsAllocated,
    DocumentsRequested,
    TasksCreated,
    Notified,
    Completed,
    Compensating,
    Compensated,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub employee_id: String,
    pub org_id: String,
    pub steps_done: Vec<String>,
    pub fail_after: Option<String>,
    pub compensation_reason: Option<String>,
}

impl State {
    pub fn start(org_id: impl Into<String>, employee_id: impl Into<String>) -> Self {
        Self {
            status: Status::Started,
            employee_id: employee_id.into(),
            org_id: org_id.into(),
            steps_done: Vec::new(),
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

    /// Advance one activity. Returns false when terminal.
    pub fn signal_advance(&mut self, step: &str) -> bool {
        if matches!(
            self.status,
            Status::Completed | Status::Compensated | Status::Failed
        ) {
            return false;
        }
        if self.status == Status::Compensating {
            self.steps_done.retain(|s| s != step && s != &format!("undo:{step}"));
            self.steps_done.push(format!("undo:{step}"));
            if self.steps_done.iter().filter(|s| s.starts_with("undo:")).count()
                >= self
                    .steps_done
                    .iter()
                    .filter(|s| !s.starts_with("undo:"))
                    .count()
                    .saturating_sub(0)
            {
                // Simplified: mark compensated once undo markers present.
            }
            return true;
        }

        let next = match (self.status, step) {
            (Status::Started, "link_user") => Status::UserLinked,
            (Status::UserLinked, "assign_role") | (Status::Started, "assign_role") => {
                Status::RoleAssigned
            }
            (Status::RoleAssigned, "allocate_assets")
            | (Status::UserLinked, "allocate_assets")
            | (Status::Started, "allocate_assets") => Status::AssetsAllocated,
            (Status::AssetsAllocated, "collect_documents") => Status::DocumentsRequested,
            (Status::DocumentsRequested, "create_tasks") => Status::TasksCreated,
            (Status::TasksCreated, "notify") => Status::Notified,
            (Status::Notified, "complete") => Status::Completed,
            // Allow skipping unused early steps when user already linked.
            (Status::Started, "create_employee") => Status::UserLinked,
            _ => return false,
        };
        self.status = next;
        self.steps_done.push(step.to_string());
        if self.maybe_fail(step) {
            return true;
        }
        true
    }

    pub fn signal_compensate(&mut self) -> bool {
        if self.status == Status::Compensating || self.compensation_reason.is_some() {
            self.status = Status::Compensated;
            true
        } else if !matches!(self.status, Status::Completed | Status::Compensated) {
            self.status = Status::Compensating;
            self.compensation_reason = Some("activity failure".into());
            true
        } else {
            false
        }
    }

    pub fn query(&self) -> &Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let mut s = State::start("org_a", "emp_b");
        assert!(s.signal_advance("create_employee"));
        assert!(s.signal_advance("assign_role"));
        assert!(s.signal_advance("allocate_assets"));
        assert!(s.signal_advance("collect_documents"));
        assert!(s.signal_advance("create_tasks"));
        assert!(s.signal_advance("notify"));
        assert!(s.signal_advance("complete"));
        assert_eq!(s.status, Status::Completed);
    }

    #[test]
    fn compensates_on_injected_failure() {
        let mut s = State::start("org_a", "emp_b").with_fail_after(Some("allocate_assets".into()));
        assert!(s.signal_advance("create_employee"));
        assert!(s.signal_advance("assign_role"));
        assert!(s.signal_advance("allocate_assets"));
        assert_eq!(s.status, Status::Compensating);
        assert!(s.signal_compensate());
        assert_eq!(s.status, Status::Compensated);
    }
}
