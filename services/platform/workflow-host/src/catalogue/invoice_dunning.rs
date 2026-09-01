//! InvoiceDunning — reminder timers for overdue invoices.
//!
//! Schedule offsets are relative to the invoice due date (negative = pre-due).
//! The default post-overdue ladder is `[3, 7, 14]` days; profiles may also
//! include a pre-due reminder at `-3`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Reminder1Sent,
    Reminder2Sent,
    FinalNoticeSent,
    Paid,
    WrittenOff,
}

/// Default post-overdue reminder offsets (days after due date).
pub const DEFAULT_SCHEDULE_OFFSETS_DAYS: &[i32] = &[3, 7, 14];

/// Classic ladder including optional pre-due reminder (matches seed profile).
pub const CLASSIC_SCHEDULE_OFFSETS_DAYS: &[i32] = &[-3, 3, 7, 14];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub invoice_id: String,
    pub reminders_sent: u32,
    /// Day offsets from due date that drive timer advancement.
    #[serde(default = "default_schedule")]
    pub schedule_offsets_days: Vec<i32>,
}

fn default_schedule() -> Vec<i32> {
    DEFAULT_SCHEDULE_OFFSETS_DAYS.to_vec()
}

impl State {
    /// Start dunning with an explicit schedule (from a dunning profile).
    pub fn start(invoice_id: impl Into<String>, schedule_offsets_days: Vec<i32>) -> Self {
        let schedule = if schedule_offsets_days.is_empty() {
            DEFAULT_SCHEDULE_OFFSETS_DAYS.to_vec()
        } else {
            schedule_offsets_days
        };
        Self {
            status: Status::Active,
            invoice_id: invoice_id.into(),
            reminders_sent: 0,
            schedule_offsets_days: schedule,
        }
    }

    /// Start with the default post-overdue ladder `[3, 7, 14]`.
    pub fn start_default(invoice_id: impl Into<String>) -> Self {
        Self::start(invoice_id, DEFAULT_SCHEDULE_OFFSETS_DAYS.to_vec())
    }

    /// Map dunning profile step offsets → schedule for [`Self::start`].
    pub fn offsets_from_steps(offset_days: impl IntoIterator<Item = i32>) -> Vec<i32> {
        offset_days.into_iter().collect()
    }

    /// Timer fired: advance through `schedule_offsets_days` dynamically.
    pub fn signal_timer(&mut self) -> bool {
        if matches!(self.status, Status::Paid | Status::WrittenOff) {
            return false;
        }
        let total = self.schedule_offsets_days.len() as u32;
        if total == 0 || self.reminders_sent >= total {
            return false;
        }
        self.reminders_sent += 1;
        self.status = if self.reminders_sent == 1 {
            Status::Reminder1Sent
        } else if self.reminders_sent >= total {
            Status::FinalNoticeSent
        } else {
            Status::Reminder2Sent
        };
        true
    }

    pub fn signal_paid(&mut self) -> bool {
        if matches!(self.status, Status::Paid | Status::WrittenOff) {
            return false;
        }
        self.status = Status::Paid;
        true
    }

    pub fn query(&self) -> &Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_profile_two_steps_advances_twice() {
        let mut s = State::start("inv_custom", vec![1, 2]);
        assert_eq!(s.status, Status::Active);
        assert!(s.signal_timer());
        assert_eq!(s.status, Status::Reminder1Sent);
        assert_eq!(s.reminders_sent, 1);
        assert!(s.signal_timer());
        assert_eq!(s.status, Status::FinalNoticeSent);
        assert_eq!(s.reminders_sent, 2);
        assert!(!s.signal_timer());
    }

    #[test]
    fn default_three_step_ladder() {
        let mut s = State::start_default("inv_abc");
        assert_eq!(s.schedule_offsets_days, vec![3, 7, 14]);
        assert!(s.signal_timer());
        assert!(s.signal_timer());
        assert!(s.signal_timer());
        assert!(!s.signal_timer());
        assert_eq!(s.reminders_sent, 3);
        assert_eq!(s.status, Status::FinalNoticeSent);
    }
}
