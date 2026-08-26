//! DataImport — staged CSV/bulk import workflow.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Validating,
    Importing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub import_id: String,
    pub rows_ok: u32,
    pub rows_failed: u32,
}

impl State {
    pub fn start(import_id: impl Into<String>) -> Self {
        Self {
            status: Status::Pending,
            import_id: import_id.into(),
            rows_ok: 0,
            rows_failed: 0,
        }
    }

    pub fn signal_validate_done(&mut self, ok: bool) -> bool {
        if self.status != Status::Pending {
            return false;
        }
        self.status = if ok {
            Status::Validating
        } else {
            Status::Failed
        };
        // After validate, move to importing when ok.
        if ok {
            self.status = Status::Importing;
        }
        true
    }

    pub fn signal_import_progress(&mut self, rows_ok: u32, rows_failed: u32) -> bool {
        if self.status != Status::Importing {
            return false;
        }
        self.rows_ok = rows_ok;
        self.rows_failed = rows_failed;
        true
    }

    pub fn signal_complete(&mut self) -> bool {
        if self.status != Status::Importing {
            return false;
        }
        self.status = Status::Completed;
        true
    }

    pub fn query(&self) -> &Self {
        self
    }
}
