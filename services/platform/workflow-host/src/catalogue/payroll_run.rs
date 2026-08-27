//! PayrollRun — Temporal workflow catalogue entry (Phase 2.3).
//!
//! Workflow id: `{org_id}:PayrollRun:{run_id}` (idempotent).
//! Long calculate / pay steps are advanced via activities that hit People APIs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Started,
    Calculating,
    Calculated,
    Paying,
    Paid,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub org_id: String,
    pub run_id: String,
    pub failure_reason: Option<String>,
}

impl State {
    pub fn start(org_id: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            status: Status::Started,
            org_id: org_id.into(),
            run_id: run_id.into(),
            failure_reason: None,
        }
    }

    pub fn workflow_id(org_id: &str, run_id: &str) -> String {
        format!("{org_id}:PayrollRun:{run_id}")
    }

    /// Advance one activity. Returns false when terminal.
    pub fn signal_advance(&mut self, step: &str) -> bool {
        if matches!(self.status, Status::Paid | Status::Failed) {
            return false;
        }
        match step {
            "calculate" => {
                self.status = Status::Calculating;
            }
            "calculated" => {
                self.status = Status::Calculated;
            }
            "pay" => {
                self.status = Status::Paying;
            }
            "paid" => {
                self.status = Status::Paid;
            }
            "fail" => {
                self.status = Status::Failed;
                self.failure_reason = Some("injected failure".into());
            }
            _ => {}
        }
        !matches!(self.status, Status::Paid | Status::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_id_stable() {
        assert_eq!(
            State::workflow_id("org_abc", "payrun_1"),
            "org_abc:PayrollRun:payrun_1"
        );
    }

    #[test]
    fn happy_path() {
        let mut s = State::start("org_x", "payrun_y");
        assert!(s.signal_advance("calculate"));
        assert_eq!(s.status, Status::Calculating);
        assert!(s.signal_advance("calculated"));
        assert!(s.signal_advance("pay"));
        assert!(!s.signal_advance("paid"));
        assert_eq!(s.status, Status::Paid);
        assert!(!s.signal_advance("calculate"));
    }
}
