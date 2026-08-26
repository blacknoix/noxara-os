//! Workflow catalogue — Phase 1.8 types hosted by companyos-workflow-host.

pub mod approval_process;
pub mod data_import;
pub mod expense_approval;
pub mod invoice_dunning;
pub mod org_provisioning;
pub mod quote_to_invoice;
pub mod tenant_deletion;
pub mod user_offboarding;

use serde::{Deserialize, Serialize};

/// Registered workflow type names (Temporal workflow type / catalogue key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkflowType {
    ApprovalProcess,
    OrgProvisioning,
    ExpenseApproval,
    QuoteToInvoice,
    InvoiceDunning,
    DataImport,
    UserOffboarding,
    TenantDeletion,
}

impl WorkflowType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApprovalProcess => "ApprovalProcess",
            Self::OrgProvisioning => "OrgProvisioning",
            Self::ExpenseApproval => "ExpenseApproval",
            Self::QuoteToInvoice => "QuoteToInvoice",
            Self::InvoiceDunning => "InvoiceDunning",
            Self::DataImport => "DataImport",
            Self::UserOffboarding => "UserOffboarding",
            Self::TenantDeletion => "TenantDeletion",
        }
    }

    pub fn all() -> &'static [WorkflowType] {
        &[
            Self::ApprovalProcess,
            Self::OrgProvisioning,
            Self::ExpenseApproval,
            Self::QuoteToInvoice,
            Self::InvoiceDunning,
            Self::DataImport,
            Self::UserOffboarding,
            Self::TenantDeletion,
        ]
    }
}

/// Build Temporal workflow id: `{org_id}:{WorkflowType}:{business_id}`.
pub fn workflow_id(org_id: &str, wf: WorkflowType, business_id: &str) -> String {
    format!("{org_id}:{}:{business_id}", wf.as_str())
}
