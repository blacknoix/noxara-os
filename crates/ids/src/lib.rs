//! Identifiers for CompanyOS.
//!
//! Internal primary keys are UUIDv7. Public API IDs are prefixed strings
//! (`org_`, `usr_`, `inv_`, `dl_`, `cus_`, …) that encode the same UUID.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use uuid::Uuid;

/// Errors when parsing a public ID.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IdError {
    #[error("invalid public id: missing or unknown prefix")]
    InvalidPrefix,
    #[error("invalid public id: bad uuid payload: {0}")]
    InvalidUuid(String),
    #[error("invalid public id: empty")]
    Empty,
}

/// Kind of public identifier (prefix without trailing underscore).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdKind {
    Org,
    User,
    Invoice,
    CreditNote,
    Payment,
    Expense,
    Deal,
    Customer,
    Contact,
    Lead,
    Quote,
    Pipeline,
    Stage,
    Product,
    Activity,
    Import,
    Project,
    Task,
    Approval,
    ApprovalPolicy,
    ApprovalDelegation,
    Hello,
    Role,
    Team,
    Department,
    Invitation,
    Membership,
    Employee,
    EmploymentContract,
    CompensationComponent,
    EmployeeDocument,
    HrAsset,
    HrTask,
    WorkSchedule,
    Holiday,
    AttendanceRecord,
    LeaveType,
    LeaveRequest,
    LeaveLedgerEntry,
    PayrollRun,
    Payslip,
    PayrollEarningLine,
    PayrollDeductionLine,
    PayrollComponent,
    /// Ledger account (`acc_`).
    LedgerAccount,
    /// Fiscal period (`period_`).
    FiscalPeriod,
    /// Bank account (`bank_`).
    BankAccount,
    /// Bank statement (`stmt_`).
    BankStatement,
    /// Bank reconciliation match (`rec_`).
    BankReconciliation,
    /// Expense policy (`pol_`).
    ExpensePolicy,
    /// Corporate card transaction (`card_`).
    CardTransaction,
    /// Reimbursement batch (`reimb_`).
    ReimbursementBatch,
    /// Warehouse (`wh_`).
    Warehouse,
    /// Inventory item (`item_`).
    InventoryItem,
    /// Stock movement (`mv_`).
    StockMovement,
    /// Supplier (`sup_`).
    Supplier,
    /// Purchase request (`pr_`).
    PurchaseRequest,
    /// Purchase request line (`prl_`).
    PurchaseRequestLine,
    /// Purchase order (`po_`).
    PurchaseOrder,
    /// Purchase order line (`poln_` — `pol_` is taken by `ExpensePolicy`).
    PurchaseOrderLine,
    /// Goods receipt note (`grn_`).
    GoodsReceipt,
    /// Goods receipt line (`grl_`).
    GoodsReceiptLine,
    /// Fixed asset (`ast_`) — inventory-owned, not HR people_asset.
    FixedAsset,
    /// Asset assignment (`asa_`).
    AssetAssignment,
    /// Maintenance schedule (`mnt_`).
    MaintenanceSchedule,
    /// Vendor bill (`vb_`) — finance-owned procure-to-pay record.
    VendorBill,
    /// SSO configuration (`sso_`).
    SsoConfig,
    /// Organization API key (`apk_`).
    ApiKey,
    /// Access review run (`arv_`).
    AccessReview,
    /// Organization secret (`sec_`).
    OrgSecret,
    /// Workflow definition (`wfd_`).
    WorkflowDefinition,
    /// Workflow definition version (`wfv_`).
    WorkflowVersion,
    /// Workflow instance run (`wfi_`).
    WorkflowInstance,
    /// Governed analytics metric definition snapshot (`met_`).
    AnalyticsMetric,
    /// Saved analytics report (`rpt_`).
    AnalyticsReport,
    /// Analytics dashboard (`adb_`).
    AnalyticsDashboard,
    /// Dashboard widget (`wdg_`).
    AnalyticsWidget,
    /// Report delivery schedule (`asch_`).
    AnalyticsSchedule,
    /// Report / export run (`arun_`).
    AnalyticsRun,
}

impl IdKind {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Org => "org_",
            Self::User => "usr_",
            Self::Invoice => "inv_",
            Self::CreditNote => "cn_",
            Self::Payment => "pay_",
            Self::Expense => "exp_",
            Self::Deal => "dl_",
            Self::Customer => "cus_",
            Self::Contact => "con_",
            Self::Lead => "led_",
            Self::Quote => "qte_",
            Self::Pipeline => "pl_",
            Self::Stage => "stg_",
            Self::Product => "prd_",
            Self::Activity => "act_",
            Self::Import => "imp_",
            Self::Project => "prj_",
            Self::Task => "tsk_",
            Self::Approval => "apr_",
            Self::ApprovalPolicy => "apl_",
            Self::ApprovalDelegation => "apd_",
            Self::Hello => "hel_",
            Self::Role => "rol_",
            Self::Team => "tem_",
            Self::Department => "dep_",
            Self::Invitation => "ivt_",
            Self::Membership => "mem_",
            Self::Employee => "emp_",
            Self::EmploymentContract => "ect_",
            Self::CompensationComponent => "cmp_",
            Self::EmployeeDocument => "edc_",
            Self::HrAsset => "has_",
            Self::HrTask => "htk_",
            Self::WorkSchedule => "sch_",
            Self::Holiday => "hol_",
            Self::AttendanceRecord => "att_",
            Self::LeaveType => "lvt_",
            Self::LeaveRequest => "lvr_",
            Self::LeaveLedgerEntry => "lv_",
            Self::PayrollRun => "payrun_",
            Self::Payslip => "payslip_",
            Self::PayrollEarningLine => "earning_",
            Self::PayrollDeductionLine => "deduction_",
            Self::PayrollComponent => "pcomp_",
            Self::LedgerAccount => "acc_",
            Self::FiscalPeriod => "period_",
            Self::BankAccount => "bank_",
            Self::BankStatement => "stmt_",
            Self::BankReconciliation => "rec_",
            Self::ExpensePolicy => "pol_",
            Self::CardTransaction => "card_",
            Self::ReimbursementBatch => "reimb_",
            Self::Warehouse => "wh_",
            Self::InventoryItem => "item_",
            Self::StockMovement => "mv_",
            Self::Supplier => "sup_",
            Self::PurchaseRequest => "pr_",
            Self::PurchaseRequestLine => "prl_",
            Self::PurchaseOrder => "po_",
            Self::PurchaseOrderLine => "poln_",
            Self::GoodsReceipt => "grn_",
            Self::GoodsReceiptLine => "grl_",
            Self::FixedAsset => "ast_",
            Self::AssetAssignment => "asa_",
            Self::MaintenanceSchedule => "mnt_",
            Self::VendorBill => "vb_",
            Self::SsoConfig => "sso_",
            Self::ApiKey => "apk_",
            Self::AccessReview => "arv_",
            Self::OrgSecret => "sec_",
            Self::WorkflowDefinition => "wfd_",
            Self::WorkflowVersion => "wfv_",
            Self::WorkflowInstance => "wfi_",
            Self::AnalyticsMetric => "met_",
            Self::AnalyticsReport => "rpt_",
            Self::AnalyticsDashboard => "adb_",
            Self::AnalyticsWidget => "wdg_",
            Self::AnalyticsSchedule => "asch_",
            Self::AnalyticsRun => "arun_",
        }
    }

    pub fn from_prefix(s: &str) -> Option<Self> {
        match s {
            "org_" => Some(Self::Org),
            "usr_" => Some(Self::User),
            "inv_" => Some(Self::Invoice),
            "cn_" => Some(Self::CreditNote),
            "payrun_" => Some(Self::PayrollRun),
            "payslip_" => Some(Self::Payslip),
            "pay_" => Some(Self::Payment),
            "exp_" => Some(Self::Expense),
            "dl_" => Some(Self::Deal),
            "cus_" => Some(Self::Customer),
            "con_" => Some(Self::Contact),
            "led_" => Some(Self::Lead),
            "qte_" => Some(Self::Quote),
            "pl_" => Some(Self::Pipeline),
            "stg_" => Some(Self::Stage),
            "prd_" => Some(Self::Product),
            "act_" => Some(Self::Activity),
            "imp_" => Some(Self::Import),
            "prj_" => Some(Self::Project),
            "tsk_" => Some(Self::Task),
            "apr_" => Some(Self::Approval),
            "apl_" => Some(Self::ApprovalPolicy),
            "apd_" => Some(Self::ApprovalDelegation),
            "hel_" => Some(Self::Hello),
            "rol_" => Some(Self::Role),
            "tem_" => Some(Self::Team),
            "dep_" => Some(Self::Department),
            "ivt_" => Some(Self::Invitation),
            "mem_" => Some(Self::Membership),
            "emp_" => Some(Self::Employee),
            "ect_" => Some(Self::EmploymentContract),
            "cmp_" => Some(Self::CompensationComponent),
            "edc_" => Some(Self::EmployeeDocument),
            "has_" => Some(Self::HrAsset),
            "htk_" => Some(Self::HrTask),
            "sch_" => Some(Self::WorkSchedule),
            "hol_" => Some(Self::Holiday),
            "att_" => Some(Self::AttendanceRecord),
            "lvt_" => Some(Self::LeaveType),
            "lvr_" => Some(Self::LeaveRequest),
            "lv_" => Some(Self::LeaveLedgerEntry),
            "earning_" => Some(Self::PayrollEarningLine),
            "deduction_" => Some(Self::PayrollDeductionLine),
            "pcomp_" => Some(Self::PayrollComponent),
            "acc_" => Some(Self::LedgerAccount),
            "period_" => Some(Self::FiscalPeriod),
            "bank_" => Some(Self::BankAccount),
            "stmt_" => Some(Self::BankStatement),
            "rec_" => Some(Self::BankReconciliation),
            "pol_" => Some(Self::ExpensePolicy),
            "card_" => Some(Self::CardTransaction),
            "reimb_" => Some(Self::ReimbursementBatch),
            "wh_" => Some(Self::Warehouse),
            "item_" => Some(Self::InventoryItem),
            "mv_" => Some(Self::StockMovement),
            "sup_" => Some(Self::Supplier),
            "pr_" => Some(Self::PurchaseRequest),
            "prl_" => Some(Self::PurchaseRequestLine),
            "po_" => Some(Self::PurchaseOrder),
            "poln_" => Some(Self::PurchaseOrderLine),
            "grn_" => Some(Self::GoodsReceipt),
            "grl_" => Some(Self::GoodsReceiptLine),
            "ast_" => Some(Self::FixedAsset),
            "asa_" => Some(Self::AssetAssignment),
            "mnt_" => Some(Self::MaintenanceSchedule),
            "vb_" => Some(Self::VendorBill),
            "sso_" => Some(Self::SsoConfig),
            "apk_" => Some(Self::ApiKey),
            "arv_" => Some(Self::AccessReview),
            "sec_" => Some(Self::OrgSecret),
            "wfd_" => Some(Self::WorkflowDefinition),
            "wfv_" => Some(Self::WorkflowVersion),
            "wfi_" => Some(Self::WorkflowInstance),
            "met_" => Some(Self::AnalyticsMetric),
            "rpt_" => Some(Self::AnalyticsReport),
            "adb_" => Some(Self::AnalyticsDashboard),
            "wdg_" => Some(Self::AnalyticsWidget),
            "asch_" => Some(Self::AnalyticsSchedule),
            "arun_" => Some(Self::AnalyticsRun),
            _ => None,
        }
    }
}

/// Generate a new UUIDv7 internal primary key.
pub fn new_uuid_v7() -> Uuid {
    Uuid::now_v7()
}

/// A typed public ID: `{prefix}{uuid}` (uuid is hyphenated lowercase).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PublicId {
    kind: IdKind,
    uuid: Uuid,
}

impl PublicId {
    pub fn new(kind: IdKind, uuid: Uuid) -> Self {
        Self { kind, uuid }
    }

    pub fn generate(kind: IdKind) -> Self {
        Self {
            kind,
            uuid: new_uuid_v7(),
        }
    }

    pub fn kind(&self) -> IdKind {
        self.kind
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    pub fn as_str(&self) -> String {
        format!("{}{}", self.kind.prefix(), self.uuid)
    }
}

impl fmt::Display for PublicId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.kind.prefix(), self.uuid)
    }
}

impl FromStr for PublicId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(IdError::Empty);
        }
        // Known prefixes (longest first where needed).
        const PREFIXES: &[(&str, IdKind)] = &[
            ("org_", IdKind::Org),
            ("usr_", IdKind::User),
            ("inv_", IdKind::Invoice),
            ("payrun_", IdKind::PayrollRun),
            ("payslip_", IdKind::Payslip),
            ("pay_", IdKind::Payment),
            ("exp_", IdKind::Expense),
            ("cus_", IdKind::Customer),
            ("con_", IdKind::Contact),
            ("led_", IdKind::Lead),
            ("qte_", IdKind::Quote),
            ("prd_", IdKind::Product),
            ("act_", IdKind::Activity),
            ("imp_", IdKind::Import),
            ("prj_", IdKind::Project),
            ("tsk_", IdKind::Task),
            ("apr_", IdKind::Approval),
            ("apl_", IdKind::ApprovalPolicy),
            ("apd_", IdKind::ApprovalDelegation),
            ("stg_", IdKind::Stage),
            ("hel_", IdKind::Hello),
            ("rol_", IdKind::Role),
            ("tem_", IdKind::Team),
            ("dep_", IdKind::Department),
            ("ivt_", IdKind::Invitation),
            ("mem_", IdKind::Membership),
            ("emp_", IdKind::Employee),
            ("ect_", IdKind::EmploymentContract),
            ("cmp_", IdKind::CompensationComponent),
            ("edc_", IdKind::EmployeeDocument),
            ("has_", IdKind::HrAsset),
            ("htk_", IdKind::HrTask),
            ("sch_", IdKind::WorkSchedule),
            ("hol_", IdKind::Holiday),
            ("att_", IdKind::AttendanceRecord),
            ("lvt_", IdKind::LeaveType),
            ("lvr_", IdKind::LeaveRequest),
            ("lv_", IdKind::LeaveLedgerEntry),
            ("earning_", IdKind::PayrollEarningLine),
            ("deduction_", IdKind::PayrollDeductionLine),
            ("pcomp_", IdKind::PayrollComponent),
            ("period_", IdKind::FiscalPeriod),
            ("acc_", IdKind::LedgerAccount),
            ("bank_", IdKind::BankAccount),
            ("stmt_", IdKind::BankStatement),
            ("rec_", IdKind::BankReconciliation),
            ("pol_", IdKind::ExpensePolicy),
            ("card_", IdKind::CardTransaction),
            ("reimb_", IdKind::ReimbursementBatch),
            ("prl_", IdKind::PurchaseRequestLine),
            ("poln_", IdKind::PurchaseOrderLine),
            ("grl_", IdKind::GoodsReceiptLine),
            ("grn_", IdKind::GoodsReceipt),
            ("asa_", IdKind::AssetAssignment),
            ("mnt_", IdKind::MaintenanceSchedule),
            ("item_", IdKind::InventoryItem),
            ("wh_", IdKind::Warehouse),
            ("mv_", IdKind::StockMovement),
            ("sup_", IdKind::Supplier),
            ("pr_", IdKind::PurchaseRequest),
            ("po_", IdKind::PurchaseOrder),
            ("ast_", IdKind::FixedAsset),
            ("vb_", IdKind::VendorBill),
            ("pl_", IdKind::Pipeline),
            ("dl_", IdKind::Deal),
            ("cn_", IdKind::CreditNote),
            ("sso_", IdKind::SsoConfig),
            ("apk_", IdKind::ApiKey),
            ("arv_", IdKind::AccessReview),
            ("sec_", IdKind::OrgSecret),
            ("asch_", IdKind::AnalyticsSchedule),
            ("arun_", IdKind::AnalyticsRun),
            ("wfd_", IdKind::WorkflowDefinition),
            ("wfv_", IdKind::WorkflowVersion),
            ("wfi_", IdKind::WorkflowInstance),
            ("met_", IdKind::AnalyticsMetric),
            ("rpt_", IdKind::AnalyticsReport),
            ("adb_", IdKind::AnalyticsDashboard),
            ("wdg_", IdKind::AnalyticsWidget),
        ];
        for (prefix, kind) in PREFIXES {
            if let Some(rest) = s.strip_prefix(prefix) {
                let uuid =
                    Uuid::parse_str(rest).map_err(|e| IdError::InvalidUuid(e.to_string()))?;
                return Ok(Self { kind: *kind, uuid });
            }
        }
        Err(IdError::InvalidPrefix)
    }
}

impl Serialize for PublicId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for PublicId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Convenience constructors.
pub fn org_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::Org, uuid)
}

pub fn usr_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::User, uuid)
}

pub fn inv_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::Invoice, uuid)
}

pub fn cn_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::CreditNote, uuid)
}

pub fn pay_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::Payment, uuid)
}

pub fn exp_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::Expense, uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v7_is_version_7() {
        let id = new_uuid_v7();
        assert_eq!(id.get_version_num(), 7);
    }

    #[test]
    fn round_trip_org() {
        let uuid = new_uuid_v7();
        let pub_id = org_id(uuid);
        assert!(pub_id.as_str().starts_with("org_"));
        let parsed: PublicId = pub_id.as_str().parse().unwrap();
        assert_eq!(parsed.kind(), IdKind::Org);
        assert_eq!(parsed.uuid(), uuid);
    }

    #[test]
    fn round_trip_usr_inv() {
        let u = new_uuid_v7();
        let usr = usr_id(u);
        assert_eq!(usr.to_string().parse::<PublicId>().unwrap(), usr);

        let inv = inv_id(u);
        assert!(inv.as_str().starts_with("inv_"));
        assert_eq!(inv.to_string().parse::<PublicId>().unwrap(), inv);
    }

    #[test]
    fn generate_and_parse_all_kinds() {
        for kind in [
            IdKind::Org,
            IdKind::User,
            IdKind::Invoice,
            IdKind::CreditNote,
            IdKind::Payment,
            IdKind::Expense,
            IdKind::Deal,
            IdKind::Customer,
            IdKind::Project,
            IdKind::Task,
            IdKind::Approval,
            IdKind::ApprovalPolicy,
            IdKind::ApprovalDelegation,
            IdKind::Hello,
            IdKind::Role,
            IdKind::Team,
            IdKind::Department,
            IdKind::Invitation,
            IdKind::Membership,
            IdKind::Employee,
            IdKind::EmploymentContract,
            IdKind::CompensationComponent,
            IdKind::EmployeeDocument,
            IdKind::HrAsset,
            IdKind::HrTask,
            IdKind::WorkSchedule,
            IdKind::Holiday,
            IdKind::AttendanceRecord,
            IdKind::LeaveType,
            IdKind::LeaveRequest,
            IdKind::LeaveLedgerEntry,
            IdKind::PayrollRun,
            IdKind::Payslip,
            IdKind::PayrollEarningLine,
            IdKind::PayrollDeductionLine,
            IdKind::PayrollComponent,
            IdKind::LedgerAccount,
            IdKind::FiscalPeriod,
            IdKind::BankAccount,
            IdKind::BankStatement,
            IdKind::BankReconciliation,
            IdKind::ExpensePolicy,
            IdKind::CardTransaction,
            IdKind::ReimbursementBatch,
            IdKind::Warehouse,
            IdKind::InventoryItem,
            IdKind::StockMovement,
            IdKind::Supplier,
            IdKind::PurchaseRequest,
            IdKind::PurchaseRequestLine,
            IdKind::PurchaseOrder,
            IdKind::PurchaseOrderLine,
            IdKind::GoodsReceipt,
            IdKind::GoodsReceiptLine,
            IdKind::FixedAsset,
            IdKind::AssetAssignment,
            IdKind::MaintenanceSchedule,
            IdKind::VendorBill,
            IdKind::SsoConfig,
            IdKind::ApiKey,
            IdKind::AccessReview,
            IdKind::OrgSecret,
            IdKind::WorkflowDefinition,
            IdKind::WorkflowVersion,
            IdKind::WorkflowInstance,
            IdKind::AnalyticsMetric,
            IdKind::AnalyticsReport,
            IdKind::AnalyticsDashboard,
            IdKind::AnalyticsWidget,
            IdKind::AnalyticsSchedule,
            IdKind::AnalyticsRun,
        ] {
            let id = PublicId::generate(kind);
            let parsed: PublicId = id.to_string().parse().unwrap();
            assert_eq!(parsed, id);
        }
    }

    #[test]
    fn serde_round_trip() {
        let id = PublicId::generate(IdKind::Org);
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains("org_"));
        let back: PublicId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn rejects_bad_prefix_and_empty() {
        assert_eq!("".parse::<PublicId>().unwrap_err(), IdError::Empty);
        assert!(matches!(
            "foo_0190".parse::<PublicId>().unwrap_err(),
            IdError::InvalidPrefix
        ));
        assert!(matches!(
            "org_not-a-uuid".parse::<PublicId>().unwrap_err(),
            IdError::InvalidUuid(_)
        ));
    }

    #[test]
    fn display_matches_as_str() {
        let id = PublicId::generate(IdKind::Deal);
        assert_eq!(id.to_string(), id.as_str());
        assert!(id.as_str().starts_with("dl_"));
    }
}
