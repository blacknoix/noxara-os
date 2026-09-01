//! AgentRun — Phase 4.3 governed autonomous agent Temporal catalogue entry.
//!
//! Workflow id shape: `{org_id}:AgentRun:{run_public_id}`.
//! Kill switch pauses / cancels in-flight AgentRun workflows for the org.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Waiting,
    Completed,
    Failed,
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub run_id: String,
    pub agent_type: String,
    pub status: Status,
    pub steps_taken: u32,
    pub kill_requested: bool,
}

impl State {
    pub fn start(run_id: impl Into<String>, agent_type: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            agent_type: agent_type.into(),
            status: Status::Running,
            steps_taken: 0,
            kill_requested: false,
        }
    }

    /// Org kill switch / signal — halt within seconds (cooperative).
    pub fn signal_kill(&mut self) {
        self.kill_requested = true;
        if matches!(self.status, Status::Running | Status::Waiting) {
            self.status = Status::Killed;
        }
    }

    pub fn step(&mut self) -> bool {
        if self.kill_requested || self.status == Status::Killed {
            self.status = Status::Killed;
            return false;
        }
        self.steps_taken += 1;
        true
    }

    pub fn complete(&mut self) {
        if !self.kill_requested {
            self.status = Status::Completed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_halts_running() {
        let mut s = State::start("agrun_1", "receivables_chase");
        assert!(s.step());
        s.signal_kill();
        assert_eq!(s.status, Status::Killed);
        assert!(!s.step());
    }
}
