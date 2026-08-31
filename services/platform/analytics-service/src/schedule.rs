//! Scheduled report delivery helpers + Temporal workflow ID shape.

use companyos_tenancy::OrgId;

/// Temporal catalogue type for Phase 3.2 scheduled delivery.
pub const WORKFLOW_TYPE: &str = "ScheduledReportDelivery";

/// `{org}:ScheduledReportDelivery:{schedule_id}:{run_id}`
pub fn temporal_workflow_id(org: OrgId, schedule_public_id: &str, run_public_id: &str) -> String {
    format!(
        "{}:{WORKFLOW_TYPE}:{schedule_public_id}:{run_public_id}",
        org.to_public()
    )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScheduleFireInput {
    pub org_id: String,
    pub schedule_id: String,
    pub report_id: String,
    pub run_id: String,
    pub export_format: String,
    pub channel: String,
}

/// Pure state machine for a delivery run (unit-tested without Temporal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    Generating,
    ExportReady,
    Notifying,
    Completed,
    Failed,
}

impl DeliveryState {
    pub fn advance(self, signal: &str) -> Self {
        match (self, signal) {
            (Self::Pending, "generate") => Self::Generating,
            (Self::Generating, "export_ready") => Self::ExportReady,
            (Self::ExportReady, "notify") => Self::Notifying,
            (Self::Notifying, "done") => Self::Completed,
            (_, "fail") => Self::Failed,
            (s, _) => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_ids::{IdKind, PublicId};

    #[test]
    fn temporal_id_includes_org_prefix() {
        let org = OrgId::generate();
        let sch = PublicId::generate(IdKind::AnalyticsSchedule);
        let run = PublicId::generate(IdKind::AnalyticsRun);
        let id = temporal_workflow_id(org, &sch.as_str(), &run.as_str());
        assert!(id.starts_with(&org.to_public().as_str()));
        assert!(id.contains("ScheduledReportDelivery"));
        assert!(id.contains(&sch.as_str()));
    }

    #[test]
    fn delivery_state_machine() {
        let mut s = DeliveryState::Pending;
        s = s.advance("generate");
        assert_eq!(s, DeliveryState::Generating);
        s = s.advance("export_ready");
        assert_eq!(s, DeliveryState::ExportReady);
        s = s.advance("notify");
        assert_eq!(s, DeliveryState::Notifying);
        s = s.advance("done");
        assert_eq!(s, DeliveryState::Completed);
    }
}
