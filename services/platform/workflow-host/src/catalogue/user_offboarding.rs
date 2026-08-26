//! UserOffboarding — revoke sessions, reassign ownership, archive.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Started,
    SessionsRevoked,
    OwnershipReassigned,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub user_id: String,
}

impl State {
    pub fn start(user_id: impl Into<String>) -> Self {
        Self {
            status: Status::Started,
            user_id: user_id.into(),
        }
    }

    pub fn signal_advance(&mut self) -> bool {
        self.status = match self.status {
            Status::Started => Status::SessionsRevoked,
            Status::SessionsRevoked => Status::OwnershipReassigned,
            Status::OwnershipReassigned => Status::Completed,
            Status::Completed => return false,
        };
        true
    }

    pub fn query(&self) -> &Self {
        self
    }
}
