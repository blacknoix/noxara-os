//! TRD 8.2 game day — Temporal down → initiating actions queue (no data loss).

use companyos_project::approvals::temporal::{start_approval_process, workflow_id};
use companyos_project::approvals::types::ApprovalProcessInput;

#[tokio::test]
async fn temporal_down_defers_start_without_data_loss() {
    std::env::set_var("TEMPORAL_ADDRESS", "127.0.0.1:9");
    let input = ApprovalProcessInput {
        org_id: "org_gameday".into(),
        approval_id: uuid::Uuid::nil().to_string(),
        approval_public_id: "apr_gameday".into(),
        sla_seconds: 3600,
        current_step: 0,
        mode: "sequential".into(),
    };
    let wf = start_approval_process(input)
        .await
        .expect("start must return Ok even when Temporal is down");
    assert_eq!(
        wf,
        workflow_id("org_gameday", "apr_gameday"),
        "workflow id reserved for later worker pickup — approval row remains source of truth"
    );
    std::env::remove_var("TEMPORAL_ADDRESS");
}
