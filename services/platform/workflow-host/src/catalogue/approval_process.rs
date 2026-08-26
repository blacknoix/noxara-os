//! ApprovalProcess — delegates to Phase 1.7 semantics (decide / SLA escalate).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Approved,
    Rejected,
    Escalated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub current_step: i32,
    pub max_step: i32,
    pub decided: bool,
}

impl State {
    pub fn start(current_step: i32, max_step: i32) -> Self {
        Self {
            status: Status::Pending,
            current_step,
            max_step,
            decided: false,
        }
    }

    pub fn signal_decide(&mut self, approve: bool) -> bool {
        if self.decided || self.status != Status::Pending {
            return false;
        }
        self.decided = true;
        self.status = if approve {
            Status::Approved
        } else {
            Status::Rejected
        };
        true
    }

    pub fn signal_sla_timeout(&mut self) -> bool {
        if self.decided || self.status != Status::Pending {
            return false;
        }
        if self.current_step < self.max_step {
            self.current_step += 1;
        } else {
            self.status = Status::Escalated;
        }
        true
    }

    pub fn query(&self) -> &Self {
        self
    }
}
