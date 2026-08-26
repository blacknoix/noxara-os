//! ExpenseApproval — thin wrapper over ApprovalProcess pattern.

use serde::{Deserialize, Serialize};

use super::approval_process;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub inner: approval_process::State,
    pub expense_id: String,
}

impl State {
    pub fn start(expense_id: impl Into<String>) -> Self {
        Self {
            inner: approval_process::State::start(1, 2),
            expense_id: expense_id.into(),
        }
    }

    pub fn signal_decide(&mut self, approve: bool) -> bool {
        self.inner.signal_decide(approve)
    }

    pub fn query(&self) -> &Self {
        self
    }
}
