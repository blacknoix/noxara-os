//! UserWorkflow — org-scoped configurable workflow instance (Phase 3.1).
//!
//! Temporal workflow id: `{org_id}:UserWorkflow:{definition_id}:{instance_id}`.
//! Durable step progression is owned by workflow-service Postgres state;
//! this catalogue state machine mirrors status for the Temporal host.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
    SlaBreached,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub definition_id: String,
    pub instance_id: String,
    pub version_number: i32,
    pub step_count: i32,
    pub max_steps: i32,
    pub current_node_id: Option<String>,
    pub error_message: Option<String>,
}

impl State {
    pub fn start(
        definition_id: impl Into<String>,
        instance_id: impl Into<String>,
        version_number: i32,
        entry_node: impl Into<String>,
        max_steps: i32,
    ) -> Self {
        Self {
            status: Status::Running,
            definition_id: definition_id.into(),
            instance_id: instance_id.into(),
            version_number,
            step_count: 0,
            max_steps,
            current_node_id: Some(entry_node.into()),
            error_message: None,
        }
    }

    /// Advance one step; returns false if terminal or capped.
    pub fn signal_step_ok(&mut self, next_node: Option<String>) -> bool {
        if self.status != Status::Running {
            return false;
        }
        if self.step_count >= self.max_steps {
            self.status = Status::Failed;
            self.error_message = Some(format!(
                "iteration/step cap exceeded ({} steps); run failed closed",
                self.max_steps
            ));
            return false;
        }
        self.step_count += 1;
        match next_node {
            None => {
                self.status = Status::Completed;
                self.current_node_id = None;
            }
            Some(n) => self.current_node_id = Some(n),
        }
        true
    }

    pub fn signal_wait(&mut self, resume_node: impl Into<String>) -> bool {
        if self.status != Status::Running {
            return false;
        }
        self.step_count += 1;
        self.current_node_id = Some(resume_node.into());
        self.status = Status::Waiting;
        true
    }

    pub fn signal_resume(&mut self) -> bool {
        if self.status != Status::Waiting {
            return false;
        }
        self.status = Status::Running;
        true
    }

    pub fn signal_fail(&mut self, message: impl Into<String>) -> bool {
        if matches!(
            self.status,
            Status::Completed | Status::Failed | Status::Cancelled
        ) {
            return false;
        }
        self.status = Status::Failed;
        self.error_message = Some(message.into());
        true
    }

    pub fn signal_cancel(&mut self) -> bool {
        if matches!(
            self.status,
            Status::Completed | Status::Failed | Status::Cancelled
        ) {
            return false;
        }
        self.status = Status::Cancelled;
        true
    }

    pub fn signal_sla_breach(&mut self) -> bool {
        if matches!(self.status, Status::Running | Status::Waiting) {
            self.status = Status::SlaBreached;
            true
        } else {
            false
        }
    }

    pub fn query(&self) -> &Self {
        self
    }
}

/// Build Temporal workflow id for a user-defined instance.
pub fn user_workflow_temporal_id(org_id: &str, definition_id: &str, instance_id: &str) -> String {
    format!("{org_id}:UserWorkflow:{definition_id}:{instance_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_cap_fails_closed() {
        let mut s = State::start("wfd_1", "wfi_1", 1, "n1", 2);
        assert!(s.signal_step_ok(Some("n1".into())));
        assert!(s.signal_step_ok(Some("n1".into())));
        assert!(!s.signal_step_ok(Some("n1".into())));
        assert_eq!(s.status, Status::Failed);
        assert!(s.error_message.as_ref().unwrap().contains("cap exceeded"));
    }

    #[test]
    fn wait_and_resume() {
        let mut s = State::start("wfd_1", "wfi_1", 1, "timer", 10);
        assert!(s.signal_wait("after"));
        assert_eq!(s.status, Status::Waiting);
        assert!(s.signal_resume());
        assert_eq!(s.status, Status::Running);
        assert!(s.signal_step_ok(None));
        assert_eq!(s.status, Status::Completed);
    }

    #[test]
    fn temporal_id_shape() {
        assert_eq!(
            user_workflow_temporal_id("org_a", "wfd_b", "wfi_c"),
            "org_a:UserWorkflow:wfd_b:wfi_c"
        );
    }
}
