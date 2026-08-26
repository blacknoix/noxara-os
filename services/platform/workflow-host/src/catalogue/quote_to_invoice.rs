//! QuoteToInvoice conversion workflow.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Started,
    CreatingInvoice,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub status: Status,
    pub quote_id: String,
    pub invoice_id: Option<String>,
}

impl State {
    pub fn start(quote_id: impl Into<String>) -> Self {
        Self {
            status: Status::Started,
            quote_id: quote_id.into(),
            invoice_id: None,
        }
    }

    pub fn signal_invoice_created(&mut self, invoice_id: impl Into<String>) -> bool {
        if self.status != Status::Started && self.status != Status::CreatingInvoice {
            return false;
        }
        self.status = Status::Completed;
        self.invoice_id = Some(invoice_id.into());
        true
    }

    pub fn signal_fail(&mut self) -> bool {
        if matches!(self.status, Status::Completed | Status::Failed) {
            return false;
        }
        self.status = Status::Failed;
        true
    }

    pub fn query(&self) -> &Self {
        self
    }
}
