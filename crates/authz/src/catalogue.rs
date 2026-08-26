//! Permission catalogue — source of truth in code.
//!
//! Rows in `permission_definition` must match [`PERMISSION_CATALOGUE`] exactly.
//! CI / `cargo test -p companyos-authz` and the workspace sync check enforce this.

use crate::{PermissionId, Scope};

/// One catalogue entry mirrored into `permission_definition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionDef {
    pub id: &'static str,
    pub context: &'static str,
    pub resource: &'static str,
    pub action: &'static str,
    pub description: &'static str,
    pub sensitive: bool,
}

impl PermissionDef {
    pub fn permission_id(&self) -> PermissionId {
        PermissionId(self.id.to_string())
    }
}

/// Full permission catalogue: Phase 1.2 Workspace, Phase 1.4 Sales (CRM), reserved Finance/Admin stubs.
pub const PERMISSION_CATALOGUE: &[PermissionDef] = &[
    PermissionDef {
        id: "workspace.dashboard.read",
        context: "workspace",
        resource: "dashboard",
        action: "read",
        description: "View workspace home / dashboard",
        sensitive: false,
    },
    PermissionDef {
        id: "workspace.org.create",
        context: "workspace",
        resource: "org",
        action: "create",
        description: "Create a new organization",
        sensitive: true,
    },
    PermissionDef {
        id: "workspace.org.read",
        context: "workspace",
        resource: "org",
        action: "read",
        description: "Read organization profile and settings",
        sensitive: false,
    },
    PermissionDef {
        id: "workspace.org.update_settings",
        context: "workspace",
        resource: "org",
        action: "update_settings",
        description: "Update organization settings, branding, locale",
        sensitive: true,
    },
    PermissionDef {
        id: "workspace.member.read",
        context: "workspace",
        resource: "member",
        action: "read",
        description: "List organization members",
        sensitive: false,
    },
    PermissionDef {
        id: "workspace.member.invite",
        context: "workspace",
        resource: "member",
        action: "invite",
        description: "Invite members to the organization",
        sensitive: true,
    },
    PermissionDef {
        id: "workspace.member.suspend",
        context: "workspace",
        resource: "member",
        action: "suspend",
        description: "Suspend a membership",
        sensitive: true,
    },
    PermissionDef {
        id: "workspace.member.revoke",
        context: "workspace",
        resource: "member",
        action: "revoke",
        description: "Revoke a membership",
        sensitive: true,
    },
    PermissionDef {
        id: "workspace.role.read",
        context: "workspace",
        resource: "role",
        action: "read",
        description: "View roles and permission matrix",
        sensitive: false,
    },
    PermissionDef {
        id: "workspace.role.manage",
        context: "workspace",
        resource: "role",
        action: "manage",
        description: "Create/edit custom roles and permission matrix",
        sensitive: true,
    },
    PermissionDef {
        id: "workspace.role.assign",
        context: "workspace",
        resource: "role",
        action: "assign",
        description: "Assign or change member roles",
        sensitive: true,
    },
    PermissionDef {
        id: "workspace.team.read",
        context: "workspace",
        resource: "team",
        action: "read",
        description: "View teams",
        sensitive: false,
    },
    PermissionDef {
        id: "workspace.team.manage",
        context: "workspace",
        resource: "team",
        action: "manage",
        description: "Create and manage teams",
        sensitive: true,
    },
    PermissionDef {
        id: "workspace.department.read",
        context: "workspace",
        resource: "department",
        action: "read",
        description: "View departments",
        sensitive: false,
    },
    PermissionDef {
        id: "workspace.department.manage",
        context: "workspace",
        resource: "department",
        action: "manage",
        description: "Create and manage departments",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.customer.read",
        context: "sales",
        resource: "customer",
        action: "read",
        description: "View customers and account details",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.customer.create",
        context: "sales",
        resource: "customer",
        action: "create",
        description: "Create new customer accounts",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.customer.update",
        context: "sales",
        resource: "customer",
        action: "update",
        description: "Update customer account details",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.customer.delete",
        context: "sales",
        resource: "customer",
        action: "delete",
        description: "Delete (soft-delete) customer accounts",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.contact.read",
        context: "sales",
        resource: "contact",
        action: "read",
        description: "View contacts linked to customers",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.contact.create",
        context: "sales",
        resource: "contact",
        action: "create",
        description: "Create contacts on customer accounts",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.contact.update",
        context: "sales",
        resource: "contact",
        action: "update",
        description: "Update contact details",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.lead.read",
        context: "sales",
        resource: "lead",
        action: "read",
        description: "View leads and lead pipeline",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.lead.create",
        context: "sales",
        resource: "lead",
        action: "create",
        description: "Create new leads",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.lead.update",
        context: "sales",
        resource: "lead",
        action: "update",
        description: "Update lead details and status",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.lead.convert",
        context: "sales",
        resource: "lead",
        action: "convert",
        description: "Convert a qualified lead to customer and deal",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.pipeline.read",
        context: "sales",
        resource: "pipeline",
        action: "read",
        description: "View sales pipelines and stages",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.pipeline.manage",
        context: "sales",
        resource: "pipeline",
        action: "manage",
        description: "Create and configure pipelines and stages",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.deal.read",
        context: "sales",
        resource: "deal",
        action: "read",
        description: "View deals and opportunity details",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.deal.create",
        context: "sales",
        resource: "deal",
        action: "create",
        description: "Create new deals",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.deal.update",
        context: "sales",
        resource: "deal",
        action: "update",
        description: "Update deal fields, stage, and amounts",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.deal.win",
        context: "sales",
        resource: "deal",
        action: "win",
        description: "Mark a deal as won",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.deal.lose",
        context: "sales",
        resource: "deal",
        action: "lose",
        description: "Mark a deal as lost",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.quote.read",
        context: "sales",
        resource: "quote",
        action: "read",
        description: "View quotes and line items",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.quote.create",
        context: "sales",
        resource: "quote",
        action: "create",
        description: "Create and draft quotes",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.quote.update",
        context: "sales",
        resource: "quote",
        action: "update",
        description: "Update quote details and line items",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.quote.accept",
        context: "sales",
        resource: "quote",
        action: "accept",
        description: "Accept a quote on behalf of the customer",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.product.read",
        context: "sales",
        resource: "product",
        action: "read",
        description: "View product catalogue",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.product.manage",
        context: "sales",
        resource: "product",
        action: "manage",
        description: "Create and manage products in the catalogue",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.activity.read",
        context: "sales",
        resource: "activity",
        action: "read",
        description: "View sales activities and notes",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.activity.create",
        context: "sales",
        resource: "activity",
        action: "create",
        description: "Log calls, meetings, emails, and notes",
        sensitive: false,
    },
    PermissionDef {
        id: "sales.import.create",
        context: "sales",
        resource: "import",
        action: "create",
        description: "Import customers, leads, and deals from files",
        sensitive: true,
    },
    PermissionDef {
        id: "sales.report.read",
        context: "sales",
        resource: "report",
        action: "read",
        description: "View sales reports and dashboards",
        sensitive: false,
    },
    PermissionDef {
        id: "admin.membership.manage",
        context: "admin",
        resource: "membership",
        action: "manage",
        description: "Admin membership management (legacy 1.1 alias)",
        sensitive: true,
    },
    PermissionDef {
        id: "admin.sso.manage",
        context: "admin",
        resource: "sso",
        action: "manage",
        description: "Manage SSO configurations",
        sensitive: true,
    },
    PermissionDef {
        id: "finance.invoice.read",
        context: "finance",
        resource: "invoice",
        action: "read",
        description: "Read invoices and invoice lines",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.invoice.create",
        context: "finance",
        resource: "invoice",
        action: "create",
        description: "Create draft invoices",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.invoice.update",
        context: "finance",
        resource: "invoice",
        action: "update",
        description: "Update draft invoices (If-Match)",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.invoice.issue",
        context: "finance",
        resource: "invoice",
        action: "issue",
        description: "Issue (finalize) invoices — immutable thereafter",
        sensitive: true,
    },
    PermissionDef {
        id: "finance.invoice.send",
        context: "finance",
        resource: "invoice",
        action: "send",
        description: "Mark invoices as sent / log payment link",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.invoice.void",
        context: "finance",
        resource: "invoice",
        action: "void",
        description: "Void an unpaid issued invoice",
        sensitive: true,
    },
    PermissionDef {
        id: "finance.invoice.approve",
        context: "finance",
        resource: "invoice",
        action: "approve",
        description: "Approve invoices requiring approval",
        sensitive: true,
    },
    PermissionDef {
        id: "finance.payment.read",
        context: "finance",
        resource: "payment",
        action: "read",
        description: "Read payments and allocations",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.payment.create",
        context: "finance",
        resource: "payment",
        action: "create",
        description: "Record payments (manual or provider webhook)",
        sensitive: true,
    },
    PermissionDef {
        id: "finance.payment.allocate",
        context: "finance",
        resource: "payment",
        action: "allocate",
        description: "Allocate payments to invoices",
        sensitive: true,
    },
    PermissionDef {
        id: "finance.credit_note.read",
        context: "finance",
        resource: "credit_note",
        action: "read",
        description: "Read credit notes",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.credit_note.create",
        context: "finance",
        resource: "credit_note",
        action: "create",
        description: "Create and issue credit notes",
        sensitive: true,
    },
    PermissionDef {
        id: "finance.expense.read",
        context: "finance",
        resource: "expense",
        action: "read",
        description: "Read expenses",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.expense.create",
        context: "finance",
        resource: "expense",
        action: "create",
        description: "Submit expenses",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.expense.approve",
        context: "finance",
        resource: "expense",
        action: "approve",
        description: "Approve or reject expenses above approval_limit",
        sensitive: true,
    },
    PermissionDef {
        id: "finance.report.read",
        context: "finance",
        resource: "report",
        action: "read",
        description: "Read finance reports and dashboard aggregates",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.customer.read",
        context: "finance",
        resource: "customer",
        action: "read",
        description: "Read finance customer projections and outstanding balances",
        sensitive: false,
    },
    PermissionDef {
        id: "finance.ledger.read",
        context: "finance",
        resource: "ledger",
        action: "read",
        description: "Read journal entries and ledger accounts",
        sensitive: false,
    },
    // --- Operations / Projects & Tasks (Phase 1.6) ---
    PermissionDef {
        id: "operations.project.read",
        context: "operations",
        resource: "project",
        action: "read",
        description: "Read projects",
        sensitive: false,
    },
    PermissionDef {
        id: "operations.project.create",
        context: "operations",
        resource: "project",
        action: "create",
        description: "Create projects (including from won deals)",
        sensitive: false,
    },
    PermissionDef {
        id: "operations.project.update",
        context: "operations",
        resource: "project",
        action: "update",
        description: "Update projects (If-Match)",
        sensitive: false,
    },
    PermissionDef {
        id: "operations.project.delete",
        context: "operations",
        resource: "project",
        action: "delete",
        description: "Soft-delete projects",
        sensitive: true,
    },
    PermissionDef {
        id: "operations.task.read",
        context: "operations",
        resource: "task",
        action: "read",
        description: "Read tasks, board, calendar, and My Work",
        sensitive: false,
    },
    PermissionDef {
        id: "operations.task.create",
        context: "operations",
        resource: "task",
        action: "create",
        description: "Create tasks",
        sensitive: false,
    },
    PermissionDef {
        id: "operations.task.update",
        context: "operations",
        resource: "task",
        action: "update",
        description: "Update tasks including board moves (If-Match)",
        sensitive: false,
    },
    PermissionDef {
        id: "operations.task.delete",
        context: "operations",
        resource: "task",
        action: "delete",
        description: "Soft-delete tasks",
        sensitive: true,
    },
    PermissionDef {
        id: "operations.task.assign",
        context: "operations",
        resource: "task",
        action: "assign",
        description: "Assign or reassign tasks",
        sensitive: false,
    },
    PermissionDef {
        id: "operations.task.comment",
        context: "operations",
        resource: "task",
        action: "comment",
        description: "Comment on tasks (including @mentions)",
        sensitive: false,
    },
    // --- Operations / Approvals (Phase 1.7) ---
    PermissionDef {
        id: "operations.approval.read",
        context: "operations",
        resource: "approval",
        action: "read",
        description: "Read approval requests and inbox",
        sensitive: false,
    },
    PermissionDef {
        id: "operations.approval.decide",
        context: "operations",
        resource: "approval",
        action: "decide",
        description: "Approve or reject pending approvals",
        sensitive: true,
    },
    PermissionDef {
        id: "operations.approval.manage",
        context: "operations",
        resource: "approval",
        action: "manage",
        description: "Create and publish versioned approval policies (policy.manage)",
        sensitive: true,
    },
    // --- Platform (Phase 1.8) ---
    PermissionDef {
        id: "platform.notification.read",
        context: "platform",
        resource: "notification",
        action: "read",
        description: "Read own in-app notification feed and preferences",
        sensitive: false,
    },
    PermissionDef {
        id: "platform.notification.manage",
        context: "platform",
        resource: "notification",
        action: "manage",
        description: "Manage notification templates and org-wide delivery settings",
        sensitive: true,
    },
    PermissionDef {
        id: "platform.search.read",
        context: "platform",
        resource: "search",
        action: "read",
        description: "Query the tenant search index",
        sensitive: false,
    },
    PermissionDef {
        id: "platform.search.reindex",
        context: "platform",
        resource: "search",
        action: "reindex",
        description: "Request a reindex job for the organization",
        sensitive: true,
    },
    PermissionDef {
        id: "platform.file.read",
        context: "platform",
        resource: "file",
        action: "read",
        description: "Read file metadata and download URLs",
        sensitive: false,
    },
    PermissionDef {
        id: "platform.file.create",
        context: "platform",
        resource: "file",
        action: "create",
        description: "Presign uploads and complete file objects",
        sensitive: false,
    },
    PermissionDef {
        id: "platform.analytics.read",
        context: "platform",
        resource: "analytics",
        action: "read",
        description: "Read analytics facts derived from the event stream",
        sensitive: false,
    },
];

/// Sensitive permission IDs used by deny-matrix DoD tests.
pub const SENSITIVE_ACTIONS: &[&str] = &[
    "workspace.member.invite",
    "workspace.role.assign",
    "workspace.member.revoke",
    "workspace.org.update_settings",
    "finance.invoice.approve",
    "finance.invoice.issue",
    "finance.invoice.void",
    "finance.payment.create",
    "finance.payment.allocate",
    "finance.credit_note.create",
    "finance.expense.approve",
    "operations.approval.decide",
    "operations.approval.manage",
];

/// All permission IDs as strings (stable sort for CI diffs).
pub fn catalogue_ids() -> Vec<&'static str> {
    let mut ids: Vec<_> = PERMISSION_CATALOGUE.iter().map(|p| p.id).collect();
    ids.sort_unstable();
    ids
}

/// Convenience constructors matching catalogue IDs.
pub mod perms {
    use super::super::PermissionId;

    pub fn workspace_dashboard_read() -> PermissionId {
        PermissionId::from("workspace.dashboard.read")
    }
    pub fn workspace_org_create() -> PermissionId {
        PermissionId::from("workspace.org.create")
    }
    pub fn workspace_org_read() -> PermissionId {
        PermissionId::from("workspace.org.read")
    }
    pub fn workspace_org_update_settings() -> PermissionId {
        PermissionId::from("workspace.org.update_settings")
    }
    pub fn workspace_member_read() -> PermissionId {
        PermissionId::from("workspace.member.read")
    }
    pub fn workspace_member_invite() -> PermissionId {
        PermissionId::from("workspace.member.invite")
    }
    pub fn workspace_member_suspend() -> PermissionId {
        PermissionId::from("workspace.member.suspend")
    }
    pub fn workspace_member_revoke() -> PermissionId {
        PermissionId::from("workspace.member.revoke")
    }
    pub fn workspace_role_read() -> PermissionId {
        PermissionId::from("workspace.role.read")
    }
    pub fn workspace_role_manage() -> PermissionId {
        PermissionId::from("workspace.role.manage")
    }
    pub fn workspace_role_assign() -> PermissionId {
        PermissionId::from("workspace.role.assign")
    }
    pub fn workspace_team_read() -> PermissionId {
        PermissionId::from("workspace.team.read")
    }
    pub fn workspace_team_manage() -> PermissionId {
        PermissionId::from("workspace.team.manage")
    }
    pub fn workspace_department_read() -> PermissionId {
        PermissionId::from("workspace.department.read")
    }
    pub fn workspace_department_manage() -> PermissionId {
        PermissionId::from("workspace.department.manage")
    }
    pub fn sales_customer_read() -> PermissionId {
        PermissionId::from("sales.customer.read")
    }
    pub fn sales_customer_create() -> PermissionId {
        PermissionId::from("sales.customer.create")
    }
    pub fn sales_customer_update() -> PermissionId {
        PermissionId::from("sales.customer.update")
    }
    pub fn sales_customer_delete() -> PermissionId {
        PermissionId::from("sales.customer.delete")
    }
    pub fn sales_contact_read() -> PermissionId {
        PermissionId::from("sales.contact.read")
    }
    pub fn sales_contact_create() -> PermissionId {
        PermissionId::from("sales.contact.create")
    }
    pub fn sales_contact_update() -> PermissionId {
        PermissionId::from("sales.contact.update")
    }
    pub fn sales_lead_read() -> PermissionId {
        PermissionId::from("sales.lead.read")
    }
    pub fn sales_lead_create() -> PermissionId {
        PermissionId::from("sales.lead.create")
    }
    pub fn sales_lead_update() -> PermissionId {
        PermissionId::from("sales.lead.update")
    }
    pub fn sales_lead_convert() -> PermissionId {
        PermissionId::from("sales.lead.convert")
    }
    pub fn sales_pipeline_read() -> PermissionId {
        PermissionId::from("sales.pipeline.read")
    }
    pub fn sales_pipeline_manage() -> PermissionId {
        PermissionId::from("sales.pipeline.manage")
    }
    pub fn sales_deal_read() -> PermissionId {
        PermissionId::from("sales.deal.read")
    }
    pub fn sales_deal_create() -> PermissionId {
        PermissionId::from("sales.deal.create")
    }
    pub fn sales_deal_update() -> PermissionId {
        PermissionId::from("sales.deal.update")
    }
    pub fn sales_deal_win() -> PermissionId {
        PermissionId::from("sales.deal.win")
    }
    pub fn sales_deal_lose() -> PermissionId {
        PermissionId::from("sales.deal.lose")
    }
    pub fn sales_quote_read() -> PermissionId {
        PermissionId::from("sales.quote.read")
    }
    pub fn sales_quote_create() -> PermissionId {
        PermissionId::from("sales.quote.create")
    }
    pub fn sales_quote_update() -> PermissionId {
        PermissionId::from("sales.quote.update")
    }
    pub fn sales_quote_accept() -> PermissionId {
        PermissionId::from("sales.quote.accept")
    }
    pub fn sales_product_read() -> PermissionId {
        PermissionId::from("sales.product.read")
    }
    pub fn sales_product_manage() -> PermissionId {
        PermissionId::from("sales.product.manage")
    }
    pub fn sales_activity_read() -> PermissionId {
        PermissionId::from("sales.activity.read")
    }
    pub fn sales_activity_create() -> PermissionId {
        PermissionId::from("sales.activity.create")
    }
    pub fn sales_import_create() -> PermissionId {
        PermissionId::from("sales.import.create")
    }
    pub fn sales_report_read() -> PermissionId {
        PermissionId::from("sales.report.read")
    }
    pub fn admin_membership_manage() -> PermissionId {
        PermissionId::from("admin.membership.manage")
    }
    pub fn admin_sso_manage() -> PermissionId {
        PermissionId::from("admin.sso.manage")
    }
    pub fn finance_invoice_approve() -> PermissionId {
        PermissionId::from("finance.invoice.approve")
    }
    pub fn finance_invoice_read() -> PermissionId {
        PermissionId::from("finance.invoice.read")
    }
    pub fn finance_invoice_create() -> PermissionId {
        PermissionId::from("finance.invoice.create")
    }
    pub fn finance_invoice_update() -> PermissionId {
        PermissionId::from("finance.invoice.update")
    }
    pub fn finance_invoice_issue() -> PermissionId {
        PermissionId::from("finance.invoice.issue")
    }
    pub fn finance_invoice_send() -> PermissionId {
        PermissionId::from("finance.invoice.send")
    }
    pub fn finance_invoice_void() -> PermissionId {
        PermissionId::from("finance.invoice.void")
    }
    pub fn finance_payment_read() -> PermissionId {
        PermissionId::from("finance.payment.read")
    }
    pub fn finance_payment_create() -> PermissionId {
        PermissionId::from("finance.payment.create")
    }
    pub fn finance_payment_allocate() -> PermissionId {
        PermissionId::from("finance.payment.allocate")
    }
    pub fn finance_credit_note_read() -> PermissionId {
        PermissionId::from("finance.credit_note.read")
    }
    pub fn finance_credit_note_create() -> PermissionId {
        PermissionId::from("finance.credit_note.create")
    }
    pub fn finance_expense_read() -> PermissionId {
        PermissionId::from("finance.expense.read")
    }
    pub fn finance_expense_create() -> PermissionId {
        PermissionId::from("finance.expense.create")
    }
    pub fn finance_expense_approve() -> PermissionId {
        PermissionId::from("finance.expense.approve")
    }
    pub fn finance_report_read() -> PermissionId {
        PermissionId::from("finance.report.read")
    }
    pub fn finance_customer_read() -> PermissionId {
        PermissionId::from("finance.customer.read")
    }
    pub fn finance_ledger_read() -> PermissionId {
        PermissionId::from("finance.ledger.read")
    }
    pub fn operations_project_read() -> PermissionId {
        PermissionId::from("operations.project.read")
    }
    pub fn operations_project_create() -> PermissionId {
        PermissionId::from("operations.project.create")
    }
    pub fn operations_project_update() -> PermissionId {
        PermissionId::from("operations.project.update")
    }
    pub fn operations_project_delete() -> PermissionId {
        PermissionId::from("operations.project.delete")
    }
    pub fn operations_task_read() -> PermissionId {
        PermissionId::from("operations.task.read")
    }
    pub fn operations_task_create() -> PermissionId {
        PermissionId::from("operations.task.create")
    }
    pub fn operations_task_update() -> PermissionId {
        PermissionId::from("operations.task.update")
    }
    pub fn operations_task_delete() -> PermissionId {
        PermissionId::from("operations.task.delete")
    }
    pub fn operations_task_assign() -> PermissionId {
        PermissionId::from("operations.task.assign")
    }
    pub fn operations_task_comment() -> PermissionId {
        PermissionId::from("operations.task.comment")
    }
    pub fn operations_approval_read() -> PermissionId {
        PermissionId::from("operations.approval.read")
    }
    pub fn operations_approval_decide() -> PermissionId {
        PermissionId::from("operations.approval.decide")
    }
    pub fn operations_approval_policy_manage() -> PermissionId {
        PermissionId::from("operations.approval.manage")
    }
    pub fn platform_notification_read() -> PermissionId {
        PermissionId::from("platform.notification.read")
    }
    pub fn platform_notification_manage() -> PermissionId {
        PermissionId::from("platform.notification.manage")
    }
    pub fn platform_search_read() -> PermissionId {
        PermissionId::from("platform.search.read")
    }
    pub fn platform_search_reindex() -> PermissionId {
        PermissionId::from("platform.search.reindex")
    }
    pub fn platform_file_read() -> PermissionId {
        PermissionId::from("platform.file.read")
    }
    pub fn platform_file_create() -> PermissionId {
        PermissionId::from("platform.file.create")
    }
    pub fn platform_analytics_read() -> PermissionId {
        PermissionId::from("platform.analytics.read")
    }
}

/// Default scope for a permission when not overridden on a role grant.
pub fn default_scope_for(permission_id: &str) -> Scope {
    match permission_id {
        "workspace.dashboard.read"
        | "workspace.org.read"
        | "workspace.member.read"
        | "workspace.role.read"
        | "workspace.team.read"
        | "workspace.department.read"
        | "sales.customer.read"
        | "sales.contact.read"
        | "sales.lead.read"
        | "sales.pipeline.read"
        | "sales.deal.read"
        | "sales.quote.read"
        | "sales.product.read"
        | "sales.activity.read"
        | "sales.report.read"
        | "finance.invoice.read"
        | "finance.payment.read"
        | "finance.credit_note.read"
        | "finance.expense.read"
        | "finance.report.read"
        | "finance.customer.read"
        | "finance.ledger.read"
        | "operations.project.read"
        | "operations.task.read"
        | "operations.approval.read"
        | "platform.notification.read"
        | "platform.search.read"
        | "platform.file.read"
        | "platform.file.create"
        | "platform.analytics.read" => Scope::Organization,
        _ => Scope::Organization,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate_permission_id;
    use std::collections::HashSet;

    #[test]
    fn catalogue_ids_are_unique_and_valid() {
        let mut seen = HashSet::new();
        for p in PERMISSION_CATALOGUE {
            assert!(
                seen.insert(p.id),
                "duplicate permission id in catalogue: {}",
                p.id
            );
            assert!(validate_permission_id(p.id).is_ok());
            assert_eq!(
                format!("{}.{}.{}", p.context, p.resource, p.action),
                p.id,
                "id must equal context.resource.action"
            );
        }
    }

    #[test]
    fn sensitive_actions_are_in_catalogue() {
        let ids: HashSet<_> = catalogue_ids().into_iter().collect();
        for s in SENSITIVE_ACTIONS {
            assert!(
                ids.contains(s),
                "sensitive action {s} missing from catalogue"
            );
        }
    }
}
