//! ScheduledReportDelivery — generate, export, and notify for a scheduled report.

use serde::{Deserialize, Serialize};

/// Durable states for a scheduled report delivery run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    Generating,
    ExportReady,
    Notifying,
    Completed,
    Failed,
}

impl DeliveryState {
    /// Advance on a workflow signal, leaving terminal and unknown transitions unchanged.
    pub fn advance(self, signal: &str) -> Self {
        match (self, signal) {
            (Self::Pending, "generate") => Self::Generating,
            (Self::Generating, "export_ready") => Self::ExportReady,
            (Self::ExportReady, "notify") => Self::Notifying,
            (Self::Notifying, "done") => Self::Completed,
            (Self::Completed | Self::Failed, _) => self,
            (_, "fail") => Self::Failed,
            (state, _) => state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DeliveryState;

    #[test]
    fn delivery_advances_to_completion() {
        let state = DeliveryState::Pending
            .advance("generate")
            .advance("export_ready")
            .advance("notify")
            .advance("done");

        assert_eq!(state, DeliveryState::Completed);
    }

    #[test]
    fn failed_and_completed_are_terminal() {
        assert_eq!(
            DeliveryState::Generating
                .advance("fail")
                .advance("generate"),
            DeliveryState::Failed
        );
        assert_eq!(
            DeliveryState::Completed.advance("fail"),
            DeliveryState::Completed
        );
    }
}
