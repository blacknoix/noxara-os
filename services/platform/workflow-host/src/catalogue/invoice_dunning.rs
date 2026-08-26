//! InvoiceDunning — reminder timers for overdue invoices.

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub invoice_id: String,
    pub reminders_sent: u32,
}

impl State {
    pub fn start(invoice_id: impl Into<String>) -> Self {
        Self {
            status: Status::Active,
            invoice_id: invoice_id.into(),
            reminders_sent: 0,
        }
    }

    /// Timer fired: advance dunning ladder.
    pub fn signal_timer(&mut self) -> bool {
        match self.status {
            Status::Active => {
                self.status = Status::Reminder1Sent;
                self.reminders_sent = 1;
                true
            }
            Status::Reminder1Sent => {
                self.status = Status::Reminder2Sent;
                self.reminders_sent = 2;
                true
            }
            Status::Reminder2Sent => {
                self.status = Status::FinalNoticeSent;
                self.reminders_sent = 3;
                true
            }
            Status::FinalNoticeSent | Status::Paid | Status::WrittenOff => false,
        }
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
