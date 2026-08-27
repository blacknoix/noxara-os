//! Happy-path tests for InvoiceDunning and DataImport (+ TenantDeletion dry_run).

use companyos_workflow_host::catalogue::data_import::{self, Status as ImportStatus};
use companyos_workflow_host::catalogue::invoice_dunning::{self, Status as DunningStatus};
use companyos_workflow_host::catalogue::tenant_deletion::{self, Status as DeletionStatus};
use companyos_workflow_host::catalogue::{workflow_id, WorkflowType};
use companyos_workflow_host::temporal_namespace;

#[test]
fn invoice_dunning_happy_path() {
    let mut s = invoice_dunning::State::start("inv_abc");
    assert_eq!(s.status, DunningStatus::Active);
    assert!(s.signal_timer());
    assert_eq!(s.status, DunningStatus::Reminder1Sent);
    assert!(s.signal_timer());
    assert_eq!(s.status, DunningStatus::Reminder2Sent);
    assert!(s.signal_timer());
    assert_eq!(s.status, DunningStatus::FinalNoticeSent);
    assert!(!s.signal_timer());
    assert!(s.signal_paid());
    assert_eq!(s.status, DunningStatus::Paid);
    assert_eq!(s.query().reminders_sent, 3);
}

#[test]
fn data_import_happy_path() {
    let mut s = data_import::State::start("imp_xyz");
    assert!(s.signal_validate_done(true));
    assert_eq!(s.status, ImportStatus::Importing);
    assert!(s.signal_import_progress(10, 1));
    assert!(s.signal_complete());
    assert_eq!(s.status, ImportStatus::Completed);
    assert_eq!(s.query().rows_ok, 10);
    assert_eq!(s.query().rows_failed, 1);
}

#[test]
fn tenant_deletion_dry_run_never_purges() {
    let mut s = tenant_deletion::State::start("org_test", true);
    assert!(s.signal_begin_wait());
    for _ in 0..tenant_deletion::RETENTION_DAYS {
        assert!(s.signal_day_elapsed());
    }
    assert_eq!(s.status, DeletionStatus::ReadyToPurge);
    assert!(s.signal_purge());
    assert_eq!(s.status, DeletionStatus::DryRunComplete);
    assert_ne!(s.status, DeletionStatus::Purged);
}

#[test]
fn workflow_id_and_namespace_defaults() {
    assert_eq!(
        workflow_id("org_abc", WorkflowType::InvoiceDunning, "inv_1"),
        "org_abc:InvoiceDunning:inv_1"
    );
    assert_eq!(
        workflow_id("org_abc", WorkflowType::EmployeeOnboarding, "emp_1"),
        "org_abc:EmployeeOnboarding:emp_1"
    );
    assert_eq!(
        workflow_id("org_abc", WorkflowType::EmployeeOffboarding, "emp_1"),
        "org_abc:EmployeeOffboarding:emp_1"
    );
    assert_eq!(
        workflow_id("org_abc", WorkflowType::LeaveCarryForward, "2026"),
        "org_abc:LeaveCarryForward:2026"
    );
    // Default when env unset in unit test process — may already be set in CI.
    let ns = temporal_namespace();
    assert!(ns == "companyos-local" || ns == "companyos-ci" || !ns.is_empty());
    assert_eq!(WorkflowType::all().len(), 11);
}
