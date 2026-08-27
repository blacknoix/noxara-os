//! LeaveCarryForward — Temporal workflow catalogue entry (Phase 2.2).
//!
//! Workflow id: `{org_id}:LeaveCarryForward:{year}` (idempotent).
//! Activities post year-end zero + capped carry-forward ledger entries via People API.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Started,
    ScanningEmployees,
    PostingEntries,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub org_id: String,
    pub year: i32,
    pub entries_posted: i32,
    pub failure_reason: Option<String>,
}

impl State {
    pub fn start(org_id: impl Into<String>, year: i32) -> Self {
        Self {
            status: Status::Started,
            org_id: org_id.into(),
            year,
            entries_posted: 0,
            failure_reason: None,
        }
    }

    pub fn workflow_id(org_id: &str, year: i32) -> String {
        format!("{org_id}:LeaveCarryForward:{year}")
    }

    /// Advance one activity. Returns false when terminal.
    pub fn signal_advance(&mut self, step: &str, entries: i32) -> bool {
        if matches!(self.status, Status::Completed | Status::Failed) {
            return false;
        }
        match step {
            "scan" => {
                self.status = Status::ScanningEmployees;
            }
            "post" => {
                self.status = Status::PostingEntries;
                self.entries_posted = entries;
            }
            "complete" => {
                self.status = Status::Completed;
            }
            "fail" => {
                self.status = Status::Failed;
                self.failure_reason = Some("injected failure".into());
            }
            _ => {}
        }
        !matches!(self.status, Status::Completed | Status::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_id_stable() {
        assert_eq!(
            State::workflow_id("org_abc", 2026),
            "org_abc:LeaveCarryForward:2026"
        );
    }

    #[test]
    fn happy_path() {
        let mut s = State::start("org_x", 2026);
        assert!(s.signal_advance("scan", 0));
        assert_eq!(s.status, Status::ScanningEmployees);
        assert!(s.signal_advance("post", 12));
        assert_eq!(s.entries_posted, 12);
        assert!(!s.signal_advance("complete", 0));
        assert_eq!(s.status, Status::Completed);
        assert!(!s.signal_advance("post", 99));
    }
}
