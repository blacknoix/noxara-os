//! TenantDeletion — 30-day soft-delete timer. Tests MUST use `dry_run`.

use serde::{Deserialize, Serialize};

/// Retention window before hard delete (days).
pub const RETENTION_DAYS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Scheduled,
    WaitingRetention,
    ReadyToPurge,
    /// Hard delete executed (never set when dry_run=true).
    Purged,
    Cancelled,
    /// dry_run completed without destroying data.
    DryRunComplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub org_id: String,
    /// When true, timer expiry transitions to DryRunComplete — never Purged.
    pub dry_run: bool,
    pub days_waited: u32,
}

impl State {
    pub fn start(org_id: impl Into<String>, dry_run: bool) -> Self {
        Self {
            status: Status::Scheduled,
            org_id: org_id.into(),
            dry_run,
            days_waited: 0,
        }
    }

    pub fn signal_begin_wait(&mut self) -> bool {
        if self.status != Status::Scheduled {
            return false;
        }
        self.status = Status::WaitingRetention;
        true
    }

    /// Simulate one day of the 30-day timer.
    pub fn signal_day_elapsed(&mut self) -> bool {
        if self.status != Status::WaitingRetention {
            return false;
        }
        self.days_waited += 1;
        if self.days_waited >= RETENTION_DAYS {
            self.status = Status::ReadyToPurge;
        }
        true
    }

    /// Execute purge activity. Honours dry_run — tests must never destroy data.
    pub fn signal_purge(&mut self) -> bool {
        if self.status != Status::ReadyToPurge {
            return false;
        }
        self.status = if self.dry_run {
            Status::DryRunComplete
        } else {
            Status::Purged
        };
        true
    }

    pub fn signal_cancel(&mut self) -> bool {
        if matches!(
            self.status,
            Status::Purged | Status::DryRunComplete | Status::Cancelled
        ) {
            return false;
        }
        self.status = Status::Cancelled;
        true
    }

    pub fn query(&self) -> &Self {
        self
    }
}
