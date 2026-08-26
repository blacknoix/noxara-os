//! OrgProvisioning durable command state machine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    SeedingRoles,
    SeedingPermissions,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub error: Option<String>,
}

impl State {
    pub fn start() -> Self {
        Self {
            status: Status::Pending,
            error: None,
        }
    }

    pub fn signal_advance(&mut self) -> bool {
        self.status = match self.status {
            Status::Pending => Status::SeedingRoles,
            Status::SeedingRoles => Status::SeedingPermissions,
            Status::SeedingPermissions => Status::Completed,
            Status::Completed | Status::Failed => return false,
        };
        true
    }

    pub fn signal_fail(&mut self, err: impl Into<String>) -> bool {
        if matches!(self.status, Status::Completed | Status::Failed) {
            return false;
        }
        self.status = Status::Failed;
        self.error = Some(err.into());
        true
    }

    pub fn query(&self) -> &Self {
        self
    }
}
