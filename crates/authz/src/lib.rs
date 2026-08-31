//! CompanyOS authorization — **the sole policy decision point (PDP)**.
//!
//! No other crate reimplements permission checks. Humans, Temporal workflows,
//! and AI all call this crate.
//!
//! # Decision order (documented, tested)
//!
//! 1. **Explicit deny** on the principal (or role) for the permission → **Deny**.
//! 2. Else **explicit allow** on the principal (or role) → **Allow**.
//! 3. Else → **Deny** (deny by default).
//!
//! Permission IDs: `{context}.{resource}.{action}`  
//! Examples: `workspace.dashboard.read`, `finance.invoice.approve`.
//!
//! Scopes: `own | team | department | organization`.

pub mod catalogue;
pub mod conditions;

pub use catalogue::{
    catalogue_ids, default_scope_for, perms, PermissionDef, PERMISSION_CATALOGUE, SENSITIVE_ACTIONS,
};
pub use conditions::{conditions_match, AbacCondition, EvaluationContext};

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A permission identifier: `{context}.{resource}.{action}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PermissionId(pub String);

impl PermissionId {
    pub fn new(context: &str, resource: &str, action: &str) -> Self {
        Self(format!("{context}.{resource}.{action}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PermissionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Resource access scope for a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Own,
    Team,
    Department,
    Organization,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Team => "team",
            Self::Department => "department",
            Self::Organization => "organization",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "own" => Some(Self::Own),
            "team" => Some(Self::Team),
            "department" => Some(Self::Department),
            "organization" => Some(Self::Organization),
            _ => None,
        }
    }

    /// Whether `self` covers the required scope (organization covers all).
    pub fn covers(self, required: Scope) -> bool {
        match self {
            Self::Organization => true,
            Self::Department => matches!(required, Scope::Department | Scope::Team | Scope::Own),
            Self::Team => matches!(required, Scope::Team | Scope::Own),
            Self::Own => required == Scope::Own,
        }
    }
}

/// Built-in system role templates (seeded per org during OrgProvisioning).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Finance,
    Sales,
    Manager,
    Member,
    ReadOnly,
}

impl Role {
    pub fn all_system() -> &'static [Role] {
        &[
            Self::Owner,
            Self::Admin,
            Self::Finance,
            Self::Sales,
            Self::Manager,
            Self::Member,
            Self::ReadOnly,
        ]
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Owner => "Owner",
            Self::Admin => "Admin",
            Self::Finance => "Finance",
            Self::Sales => "Sales",
            Self::Manager => "Manager",
            Self::Member => "Member",
            Self::ReadOnly => "Read-only",
        }
    }

    /// Default allows for the role (explicit denials still win).
    pub fn default_allows(self) -> HashSet<PermissionId> {
        let all: HashSet<PermissionId> = PERMISSION_CATALOGUE
            .iter()
            .map(|p| p.permission_id())
            .collect();
        match self {
            Self::Owner => all,
            Self::Admin => all
                .into_iter()
                .filter(|p| p.as_str() != "workspace.org.create")
                .collect(),
            Self::Finance => HashSet::from([
                perms::workspace_dashboard_read(),
                perms::workspace_org_read(),
                perms::workspace_member_read(),
                perms::workspace_role_read(),
                perms::workspace_team_read(),
                perms::workspace_department_read(),
                perms::finance_invoice_read(),
                perms::finance_invoice_create(),
                perms::finance_invoice_update(),
                perms::finance_invoice_issue(),
                perms::finance_invoice_send(),
                perms::finance_invoice_void(),
                perms::finance_invoice_approve(),
                perms::finance_payment_read(),
                perms::finance_payment_create(),
                perms::finance_payment_allocate(),
                perms::finance_credit_note_read(),
                perms::finance_credit_note_create(),
                perms::finance_expense_read(),
                perms::finance_expense_create(),
                perms::finance_expense_approve(),
                perms::finance_report_read(),
                perms::finance_customer_read(),
                perms::finance_ledger_read(),
                perms::operations_approval_read(),
                perms::operations_approval_decide(),
                perms::operations_workflow_read(),
                perms::platform_notification_read(),
                perms::platform_search_read(),
                perms::platform_file_read(),
                perms::platform_file_create(),
                perms::platform_analytics_read(),
                perms::analytics_report_read(),
                perms::analytics_report_write(),
                perms::analytics_report_run(),
                perms::analytics_report_share(),
                perms::analytics_report_export(),
                perms::analytics_dashboard_read(),
                perms::analytics_dashboard_write(),
                perms::ai_copilot_use(),
                perms::ai_proposal_create(),
                perms::ai_proposal_commit(),
                perms::ai_settings_read(),
                perms::ai_insights_read(),
                perms::ai_document_extract(),
                perms::hr_employee_read(),
                perms::hr_employee_read_sensitive(),
                perms::hr_field_compensation_read(),
                perms::hr_field_government_id_read(),
                perms::hr_field_bank_read(),
                perms::hr_document_read(),
                perms::hr_attendance_read(),
                perms::hr_leave_read(),
                perms::hr_payroll_read(),
                perms::hr_payroll_approve(),
                perms::finance_journal_post(),
                perms::finance_account_manage(),
                perms::finance_period_read(),
                perms::finance_period_close(),
                perms::finance_period_reopen(),
                perms::finance_bank_read(),
                perms::finance_bank_reconcile(),
                perms::finance_field_bank_account_read(),
                perms::finance_field_salary_journal_read(),
                perms::finance_expense_policy_manage(),
                perms::finance_reimbursement_manage(),
                perms::inventory_item_read(),
                perms::inventory_warehouse_read(),
                perms::inventory_stock_read(),
                perms::inventory_supplier_read(),
                perms::inventory_purchase_request_read(),
                perms::inventory_purchase_order_read(),
                perms::inventory_goods_receipt_read(),
                perms::inventory_asset_read(),
            ]),
            Self::Sales => HashSet::from([
                perms::workspace_dashboard_read(),
                perms::workspace_org_read(),
                perms::workspace_member_read(),
                perms::workspace_role_read(),
                perms::workspace_team_read(),
                perms::workspace_department_read(),
                perms::sales_customer_read(),
                perms::sales_customer_create(),
                perms::sales_customer_update(),
                perms::sales_contact_read(),
                perms::sales_contact_create(),
                perms::sales_contact_update(),
                perms::sales_lead_read(),
                perms::sales_lead_create(),
                perms::sales_lead_update(),
                perms::sales_lead_convert(),
                perms::sales_pipeline_read(),
                perms::sales_pipeline_manage(),
                perms::sales_deal_read(),
                perms::sales_deal_create(),
                perms::sales_deal_update(),
                perms::sales_deal_win(),
                perms::sales_deal_lose(),
                perms::sales_quote_read(),
                perms::sales_quote_create(),
                perms::sales_quote_update(),
                perms::sales_quote_accept(),
                perms::sales_product_read(),
                perms::sales_product_manage(),
                perms::sales_activity_read(),
                perms::sales_activity_create(),
                perms::sales_import_create(),
                perms::sales_report_read(),
                perms::finance_customer_read(),
                perms::finance_invoice_read(),
                perms::finance_invoice_create(),
                perms::operations_project_read(),
                perms::operations_project_create(),
                perms::operations_task_read(),
                perms::operations_task_create(),
                perms::operations_task_update(),
                perms::operations_task_comment(),
                perms::operations_approval_read(),
                perms::operations_workflow_read(),
                perms::operations_workflow_write(),
                perms::operations_workflow_run(),
                perms::platform_notification_read(),
                perms::platform_search_read(),
                perms::platform_file_read(),
                perms::platform_file_create(),
                perms::platform_analytics_read(),
                perms::analytics_report_read(),
                perms::analytics_report_run(),
                perms::analytics_dashboard_read(),
                perms::ai_copilot_use(),
                perms::ai_proposal_create(),
                perms::ai_proposal_commit(),
                perms::ai_settings_read(),
                perms::ai_insights_read(),
                perms::ai_document_extract(),
                perms::hr_employee_read(),
                perms::hr_attendance_read(),
                perms::hr_leave_read(),
            ]),
            Self::Manager => HashSet::from([
                perms::workspace_dashboard_read(),
                perms::workspace_org_read(),
                perms::workspace_member_read(),
                perms::workspace_member_invite(),
                perms::workspace_role_read(),
                perms::workspace_team_read(),
                perms::workspace_team_manage(),
                perms::workspace_department_read(),
                perms::admin_membership_manage(),
                perms::sales_customer_read(),
                perms::sales_contact_read(),
                perms::sales_lead_read(),
                perms::sales_pipeline_read(),
                perms::sales_deal_read(),
                perms::sales_deal_update(),
                perms::sales_quote_read(),
                perms::sales_product_read(),
                perms::sales_activity_read(),
                perms::sales_report_read(),
                perms::finance_invoice_read(),
                perms::finance_expense_read(),
                perms::finance_expense_create(),
                perms::finance_report_read(),
                perms::finance_customer_read(),
                perms::operations_project_read(),
                perms::operations_project_create(),
                perms::operations_project_update(),
                perms::operations_project_delete(),
                perms::operations_task_read(),
                perms::operations_task_create(),
                perms::operations_task_update(),
                perms::operations_task_delete(),
                perms::operations_task_assign(),
                perms::operations_task_comment(),
                perms::operations_approval_read(),
                perms::operations_approval_decide(),
                perms::operations_workflow_read(),
                perms::operations_workflow_write(),
                perms::operations_workflow_run(),
                perms::platform_notification_read(),
                perms::platform_search_read(),
                perms::platform_search_reindex(),
                perms::platform_file_read(),
                perms::platform_file_create(),
                perms::platform_analytics_read(),
                perms::analytics_report_read(),
                perms::analytics_report_write(),
                perms::analytics_report_run(),
                perms::analytics_report_share(),
                perms::analytics_report_export(),
                perms::analytics_dashboard_read(),
                perms::analytics_dashboard_write(),
                perms::ai_copilot_use(),
                perms::ai_proposal_create(),
                perms::ai_proposal_commit(),
                perms::ai_settings_read(),
                perms::ai_insights_read(),
                perms::ai_document_extract(),
                perms::hr_employee_read(),
                perms::hr_employee_read_sensitive(),
                perms::hr_field_compensation_read(),
                perms::hr_field_government_id_read(),
                perms::hr_field_bank_read(),
                perms::hr_employee_write(),
                perms::hr_employee_onboard(),
                perms::hr_employee_offboard(),
                perms::hr_document_read(),
                perms::hr_document_write(),
                perms::hr_attendance_read(),
                perms::hr_attendance_write(),
                perms::hr_leave_read(),
                perms::hr_leave_write(),
                perms::hr_leave_approve(),
                perms::hr_payroll_read(),
                perms::hr_payroll_write(),
                perms::hr_payroll_approve(),
                perms::hr_payroll_run(),
                perms::finance_journal_post(),
                perms::finance_period_read(),
                perms::finance_bank_read(),
                perms::inventory_item_read(),
                perms::inventory_item_write(),
                perms::inventory_warehouse_read(),
                perms::inventory_warehouse_write(),
                perms::inventory_stock_read(),
                perms::inventory_stock_move(),
                perms::inventory_supplier_read(),
                perms::inventory_supplier_write(),
                perms::inventory_purchase_request_read(),
                perms::inventory_purchase_request_write(),
                perms::inventory_purchase_order_read(),
                perms::inventory_purchase_order_write(),
                perms::inventory_goods_receipt_read(),
                perms::inventory_goods_receipt_write(),
                perms::inventory_asset_read(),
                perms::inventory_asset_write(),
            ]),
            Self::Member => HashSet::from([
                perms::workspace_dashboard_read(),
                perms::workspace_org_read(),
                perms::workspace_member_read(),
                perms::workspace_role_read(),
                perms::workspace_team_read(),
                perms::workspace_department_read(),
                perms::sales_customer_read(),
                perms::sales_contact_read(),
                perms::sales_lead_read(),
                perms::sales_pipeline_read(),
                perms::sales_deal_read(),
                perms::sales_quote_read(),
                perms::sales_product_read(),
                perms::sales_activity_read(),
                perms::sales_report_read(),
                perms::finance_expense_read(),
                perms::finance_expense_create(),
                perms::operations_project_read(),
                perms::operations_task_read(),
                perms::operations_task_create(),
                perms::operations_task_comment(),
                perms::operations_approval_read(),
                perms::operations_workflow_read(),
                perms::operations_workflow_write(),
                perms::operations_workflow_run(),
                perms::platform_notification_read(),
                perms::platform_search_read(),
                perms::platform_file_read(),
                perms::platform_file_create(),
                perms::analytics_report_read(),
                perms::analytics_report_run(),
                perms::analytics_dashboard_read(),
                perms::ai_copilot_use(),
                perms::ai_proposal_create(),
                perms::ai_proposal_commit(),
                perms::ai_settings_read(),
                perms::ai_insights_read(),
                perms::ai_document_extract(),
                perms::hr_employee_read(),
                perms::hr_document_read(),
                perms::hr_attendance_read(),
                perms::hr_attendance_write(),
                perms::hr_leave_read(),
                perms::hr_leave_write(),
                perms::inventory_item_read(),
                perms::inventory_warehouse_read(),
                perms::inventory_stock_read(),
                perms::inventory_purchase_request_read(),
                perms::inventory_purchase_request_write(),
                perms::admin_marketplace_read(),
            ]),
            Self::ReadOnly => HashSet::from([
                perms::workspace_dashboard_read(),
                perms::workspace_org_read(),
                perms::workspace_member_read(),
                perms::workspace_role_read(),
                perms::workspace_team_read(),
                perms::workspace_department_read(),
                perms::sales_customer_read(),
                perms::sales_contact_read(),
                perms::sales_lead_read(),
                perms::sales_pipeline_read(),
                perms::sales_deal_read(),
                perms::sales_quote_read(),
                perms::sales_product_read(),
                perms::sales_activity_read(),
                perms::sales_report_read(),
                perms::finance_invoice_read(),
                perms::finance_report_read(),
                perms::finance_customer_read(),
                perms::operations_project_read(),
                perms::operations_task_read(),
                perms::operations_approval_read(),
                perms::operations_workflow_read(),
                perms::platform_notification_read(),
                perms::platform_search_read(),
                perms::platform_file_read(),
                perms::platform_analytics_read(),
                perms::analytics_report_read(),
                perms::analytics_report_run(),
                perms::analytics_dashboard_read(),
                perms::ai_copilot_use(),
                perms::ai_settings_read(),
                perms::ai_insights_read(),
                perms::hr_employee_read(),
                perms::hr_document_read(),
                perms::hr_attendance_read(),
                perms::hr_leave_read(),
                perms::inventory_item_read(),
                perms::inventory_warehouse_read(),
                perms::inventory_stock_read(),
                perms::inventory_supplier_read(),
                perms::inventory_purchase_request_read(),
                perms::inventory_purchase_order_read(),
                perms::inventory_goods_receipt_read(),
                perms::inventory_asset_read(),
                perms::admin_marketplace_read(),
            ]),
        }
    }

    /// Owner and Admin require TOTP MFA before an access token is issued.
    pub fn requires_mfa(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "finance" => Some(Self::Finance),
            "sales" => Some(Self::Sales),
            "manager" => Some(Self::Manager),
            "member" => Some(Self::Member),
            "read_only" | "readonly" | "read-only" => Some(Self::ReadOnly),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Finance => "finance",
            Self::Sales => "sales",
            Self::Manager => "manager",
            Self::Member => "member",
            Self::ReadOnly => "read_only",
        }
    }

    /// Whether this role is the Owner template (last-owner invariant).
    pub fn is_owner(self) -> bool {
        matches!(self, Self::Owner)
    }
}

/// Returns true if any role on the principal requires MFA.
pub fn principal_requires_mfa(principal: &Principal) -> bool {
    principal.roles.iter().any(|r| r.requires_mfa())
}

/// Effect of a policy statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Allow,
    Deny,
}

/// A single policy statement attached to a principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    pub effect: Effect,
    pub permission: PermissionId,
    #[serde(default = "default_statement_scope")]
    pub scope: Scope,
    /// Optional ABAC predicates (AND). Empty = no extra constraints.
    #[serde(default)]
    pub conditions: Vec<AbacCondition>,
}

fn default_statement_scope() -> Scope {
    Scope::Organization
}

impl Statement {
    pub fn allow(permission: PermissionId) -> Self {
        Self {
            effect: Effect::Allow,
            permission,
            scope: Scope::Organization,
            conditions: vec![],
        }
    }

    pub fn deny(permission: PermissionId) -> Self {
        Self {
            effect: Effect::Deny,
            permission,
            scope: Scope::Organization,
            conditions: vec![],
        }
    }

    pub fn with_conditions(mut self, conditions: Vec<AbacCondition>) -> Self {
        self.conditions = conditions;
        self
    }
}

/// Principal under evaluation (user within an org, with roles + statements).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub roles: Vec<Role>,
    /// Explicit statements (allow or deny). Deny wins over role defaults and allows.
    pub statements: Vec<Statement>,
}

impl Principal {
    pub fn with_roles(roles: Vec<Role>) -> Self {
        Self {
            roles,
            statements: vec![],
        }
    }

    /// Build a principal that only allows the given permission IDs (no role defaults).
    /// Used for organization API keys after scopes ∩ owner-role intersection.
    pub fn from_permission_ids<I, S>(ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let statements = ids
            .into_iter()
            .map(|s| Statement {
                effect: Effect::Allow,
                permission: PermissionId::from(s.as_ref()),
                scope: Scope::Organization,
                conditions: vec![],
            })
            .collect();
        Self {
            roles: vec![],
            statements,
        }
    }

    /// Restrict this principal to the intersection of its current allows and `scopes`.
    /// Empty `scopes` yields a principal that denies everything.
    pub fn intersect_scopes(&self, scopes: &[String]) -> Self {
        let allowed: HashSet<PermissionId> = scopes
            .iter()
            .map(|s| PermissionId::from(s.as_str()))
            .filter(|p| is_allowed(self, p))
            .collect();
        Self::from_permission_ids(allowed.iter().map(|p| p.as_str().to_string()))
    }

    /// Effective allowed permission IDs (for capability preview / nav).
    pub fn effective_allows(&self) -> HashSet<PermissionId> {
        let mut out = HashSet::new();
        for p in PERMISSION_CATALOGUE {
            let pid = p.permission_id();
            if is_allowed(self, &pid) {
                out.insert(pid);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionDetail {
    pub decision: Decision,
    pub permission: PermissionId,
    pub reason: &'static str,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthzError {
    #[error("invalid permission id: {0}")]
    InvalidPermission(String),
}

/// Validate permission id shape: exactly three dotted segments, non-empty.
pub fn validate_permission_id(id: &str) -> Result<(), AuthzError> {
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(AuthzError::InvalidPermission(id.to_string()));
    }
    Ok(())
}

/// Sole PDP entrypoint.
///
/// Decision order:
/// 1. Explicit deny in `principal.statements` → Deny  
/// 2. Explicit allow in `principal.statements` (scope + ABAC) → Allow  
/// 3. Allow from any role default → Allow  
/// 4. Otherwise → Deny
pub fn decide(principal: &Principal, permission: &PermissionId) -> DecisionDetail {
    decide_with_context(
        principal,
        permission,
        Scope::Organization,
        &EvaluationContext::default(),
    )
}

/// PDP with a required resource scope (empty ABAC context).
pub fn decide_with_scope(
    principal: &Principal,
    permission: &PermissionId,
    required_scope: Scope,
) -> DecisionDetail {
    decide_with_context(
        principal,
        permission,
        required_scope,
        &EvaluationContext::default(),
    )
}

/// PDP with scope + ABAC evaluation context.
pub fn decide_with_context(
    principal: &Principal,
    permission: &PermissionId,
    required_scope: Scope,
    ctx: &EvaluationContext,
) -> DecisionDetail {
    // 1. Explicit deny wins (any scope). Deny statements ignore ABAC so
    // operators can always cut access.
    if principal
        .statements
        .iter()
        .any(|s| s.effect == Effect::Deny && s.permission == *permission)
    {
        return DecisionDetail {
            decision: Decision::Deny,
            permission: permission.clone(),
            reason: "explicit_deny",
        };
    }

    // 2. Explicit allow with sufficient scope + satisfied conditions.
    if principal.statements.iter().any(|s| {
        s.effect == Effect::Allow
            && s.permission == *permission
            && s.scope.covers(required_scope)
            && conditions_match(&s.conditions, ctx)
    }) {
        return DecisionDetail {
            decision: Decision::Allow,
            permission: permission.clone(),
            reason: "explicit_allow",
        };
    }

    // Explicit allow that failed only on ABAC → distinct reason for operators.
    if principal.statements.iter().any(|s| {
        s.effect == Effect::Allow
            && s.permission == *permission
            && s.scope.covers(required_scope)
            && !s.conditions.is_empty()
            && !conditions_match(&s.conditions, ctx)
    }) {
        return DecisionDetail {
            decision: Decision::Deny,
            permission: permission.clone(),
            reason: "abac_deny",
        };
    }

    // 3. Role defaults (organization scope). Role templates have no ABAC
    // conditions; handlers that need ABAC must attach statements.
    if required_scope == Scope::Organization || Scope::Organization.covers(required_scope) {
        for role in &principal.roles {
            if role.default_allows().contains(permission) {
                return DecisionDetail {
                    decision: Decision::Allow,
                    permission: permission.clone(),
                    reason: "role_allow",
                };
            }
        }
    }

    // 4. Deny by default.
    DecisionDetail {
        decision: Decision::Deny,
        permission: permission.clone(),
        reason: "default_deny",
    }
}

pub fn is_allowed(principal: &Principal, permission: &PermissionId) -> bool {
    decide(principal, permission).decision == Decision::Allow
}

pub fn is_allowed_scoped(
    principal: &Principal,
    permission: &PermissionId,
    required_scope: Scope,
) -> bool {
    decide_with_scope(principal, permission, required_scope).decision == Decision::Allow
}

pub fn is_allowed_with_context(
    principal: &Principal,
    permission: &PermissionId,
    required_scope: Scope,
    ctx: &EvaluationContext,
) -> bool {
    decide_with_context(principal, permission, required_scope, ctx).decision == Decision::Allow
}

/// Short-lived cache of effective permission sets keyed by membership + policy_version.
///
/// Invalidation: callers bump `policy_version` on role/membership/settings changes;
/// entries whose key no longer matches are unused. TTL enforces ≤5s stale reads.
#[derive(Debug, Default)]
pub struct PermissionSetCache {
    inner: std::sync::Mutex<std::collections::HashMap<CacheKey, CacheEntry>>,
    ttl: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    membership_id: String,
    policy_version: i64,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    allows: HashSet<String>,
    inserted_at: Instant,
}

impl PermissionSetCache {
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: std::sync::Mutex::new(std::collections::HashMap::new()),
            ttl,
        }
    }

    pub fn default_5s() -> Self {
        Self::with_ttl(Duration::from_secs(5))
    }

    pub fn get(&self, membership_id: &str, policy_version: i64) -> Option<HashSet<String>> {
        let mut guard = self.inner.lock().ok()?;
        let key = CacheKey {
            membership_id: membership_id.to_string(),
            policy_version,
        };
        let entry = guard.get(&key)?;
        if entry.inserted_at.elapsed() > self.ttl {
            guard.remove(&key);
            return None;
        }
        Some(entry.allows.clone())
    }

    pub fn put(&self, membership_id: &str, policy_version: i64, allows: HashSet<String>) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(
                CacheKey {
                    membership_id: membership_id.to_string(),
                    policy_version,
                },
                CacheEntry {
                    allows,
                    inserted_at: Instant::now(),
                },
            );
        }
    }

    /// Drop all entries for a membership (any policy_version).
    pub fn invalidate_membership(&self, membership_id: &str) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.retain(|k, _| k.membership_id != membership_id);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_can_approve_invoice() {
        let p = Principal::with_roles(vec![Role::Owner]);
        assert!(is_allowed(&p, &perms::finance_invoice_approve()));
        assert!(is_allowed(&p, &perms::workspace_dashboard_read()));
    }

    #[test]
    fn member_can_read_dashboard_not_approve() {
        let p = Principal::with_roles(vec![Role::Member]);
        assert!(is_allowed(&p, &perms::workspace_dashboard_read()));
        assert!(!is_allowed(&p, &perms::finance_invoice_approve()));
        let d = decide(&p, &perms::finance_invoice_approve());
        assert_eq!(d.decision, Decision::Deny);
        assert_eq!(d.reason, "default_deny");
    }

    #[test]
    fn deny_by_default_unknown_permission() {
        let p = Principal::with_roles(vec![Role::Owner]);
        let unknown = PermissionId::from("ops.widget.fly");
        assert!(!is_allowed(&p, &unknown));
    }

    #[test]
    fn explicit_deny_wins_over_owner_role() {
        let mut p = Principal::with_roles(vec![Role::Owner]);
        p.statements
            .push(Statement::deny(perms::finance_invoice_approve()));
        let d = decide(&p, &perms::finance_invoice_approve());
        assert_eq!(d.decision, Decision::Deny);
        assert_eq!(d.reason, "explicit_deny");
    }

    #[test]
    fn explicit_deny_wins_over_explicit_allow() {
        let mut p = Principal::with_roles(vec![]);
        p.statements
            .push(Statement::allow(perms::workspace_dashboard_read()));
        p.statements
            .push(Statement::deny(perms::workspace_dashboard_read()));
        assert!(!is_allowed(&p, &perms::workspace_dashboard_read()));
    }

    #[test]
    fn explicit_allow_for_member_invoice() {
        let mut p = Principal::with_roles(vec![Role::Member]);
        assert!(!is_allowed(&p, &perms::finance_invoice_approve()));
        p.statements
            .push(Statement::allow(perms::finance_invoice_approve()));
        let d = decide(&p, &perms::finance_invoice_approve());
        assert_eq!(d.decision, Decision::Allow);
        assert_eq!(d.reason, "explicit_allow");
    }

    #[test]
    fn empty_principal_denied() {
        let p = Principal::with_roles(vec![]);
        assert!(!is_allowed(&p, &perms::workspace_dashboard_read()));
    }

    #[test]
    fn permission_id_format() {
        assert!(validate_permission_id("workspace.dashboard.read").is_ok());
        assert!(validate_permission_id("finance.invoice.approve").is_ok());
        assert!(validate_permission_id("too.few").is_err());
        assert!(validate_permission_id("a.b.c.d").is_err());
        assert!(validate_permission_id(".b.c").is_err());
    }

    #[test]
    fn decision_order_documented_in_reasons() {
        let mut p = Principal::with_roles(vec![Role::Owner]);
        p.statements
            .push(Statement::deny(perms::workspace_dashboard_read()));
        assert_eq!(
            decide(&p, &perms::workspace_dashboard_read()).reason,
            "explicit_deny"
        );

        let p = Principal::with_roles(vec![Role::Member]);
        assert_eq!(
            decide(&p, &perms::workspace_dashboard_read()).reason,
            "role_allow"
        );
    }

    #[test]
    fn owner_and_admin_require_mfa() {
        assert!(Role::Owner.requires_mfa());
        assert!(Role::Admin.requires_mfa());
        assert!(!Role::Member.requires_mfa());
        assert!(!Role::Finance.requires_mfa());
        assert!(principal_requires_mfa(&Principal::with_roles(vec![
            Role::Owner
        ])));
        assert!(principal_requires_mfa(&Principal::with_roles(vec![
            Role::Admin
        ])));
        assert!(!principal_requires_mfa(&Principal::with_roles(vec![
            Role::Member
        ])));
    }

    #[test]
    fn admin_can_manage_sso() {
        let p = Principal::with_roles(vec![Role::Admin]);
        assert!(is_allowed(&p, &perms::admin_sso_manage()));
        let member = Principal::with_roles(vec![Role::Member]);
        assert!(!is_allowed(&member, &perms::admin_sso_manage()));
    }

    #[test]
    fn system_role_deny_matrix_for_sensitive_actions() {
        // Every non-owner system role × every sensitive action that role must not have by default.
        let deny_pairs: &[(&str, Role)] = &[
            ("workspace.member.invite", Role::Member),
            ("workspace.member.invite", Role::ReadOnly),
            ("workspace.member.invite", Role::Finance),
            ("workspace.member.invite", Role::Sales),
            ("workspace.role.assign", Role::Member),
            ("workspace.role.assign", Role::ReadOnly),
            ("workspace.role.assign", Role::Finance),
            ("workspace.role.assign", Role::Sales),
            ("workspace.role.assign", Role::Manager),
            ("workspace.member.revoke", Role::Member),
            ("workspace.member.revoke", Role::ReadOnly),
            ("workspace.member.revoke", Role::Finance),
            ("workspace.member.revoke", Role::Sales),
            ("workspace.member.revoke", Role::Manager),
            ("workspace.org.update_settings", Role::Member),
            ("workspace.org.update_settings", Role::ReadOnly),
            ("workspace.org.update_settings", Role::Finance),
            ("workspace.org.update_settings", Role::Sales),
            ("workspace.org.update_settings", Role::Manager),
            ("finance.invoice.approve", Role::Member),
            ("finance.invoice.approve", Role::ReadOnly),
            ("finance.invoice.approve", Role::Sales),
            ("finance.invoice.approve", Role::Manager),
            ("finance.invoice.issue", Role::Member),
            ("finance.invoice.issue", Role::ReadOnly),
            ("finance.invoice.issue", Role::Sales),
            ("finance.invoice.issue", Role::Manager),
            ("finance.invoice.void", Role::Member),
            ("finance.invoice.void", Role::ReadOnly),
            ("finance.invoice.void", Role::Sales),
            ("finance.invoice.void", Role::Manager),
            ("finance.payment.create", Role::Member),
            ("finance.payment.create", Role::ReadOnly),
            ("finance.payment.create", Role::Sales),
            ("finance.payment.create", Role::Manager),
            ("finance.expense.approve", Role::Member),
            ("finance.expense.approve", Role::ReadOnly),
            ("finance.expense.approve", Role::Sales),
            ("finance.expense.approve", Role::Manager),
            ("operations.approval.decide", Role::Member),
            ("operations.approval.decide", Role::ReadOnly),
            ("operations.approval.decide", Role::Sales),
            ("operations.approval.manage", Role::Member),
            ("operations.approval.manage", Role::ReadOnly),
            ("operations.approval.manage", Role::Sales),
            ("operations.approval.manage", Role::Finance),
            ("operations.approval.manage", Role::Manager),
            ("operations.workflow.manage", Role::Member),
            ("operations.workflow.manage", Role::ReadOnly),
            ("operations.workflow.manage", Role::Sales),
            ("operations.workflow.manage", Role::Finance),
            ("operations.workflow.manage", Role::Manager),
            ("operations.workflow.publish", Role::Member),
            ("operations.workflow.publish", Role::ReadOnly),
            ("operations.workflow.publish", Role::Sales),
            ("operations.workflow.publish", Role::Finance),
            ("operations.workflow.publish", Role::Manager),
            ("hr.payroll.read", Role::Member),
            ("hr.payroll.read", Role::ReadOnly),
            ("hr.payroll.read", Role::Sales),
            ("hr.payroll.write", Role::Member),
            ("hr.payroll.write", Role::ReadOnly),
            ("hr.payroll.write", Role::Sales),
            ("hr.payroll.write", Role::Finance),
            ("hr.payroll.approve", Role::Member),
            ("hr.payroll.approve", Role::ReadOnly),
            ("hr.payroll.approve", Role::Sales),
            ("hr.payroll.run", Role::Member),
            ("hr.payroll.run", Role::ReadOnly),
            ("hr.payroll.run", Role::Sales),
            ("finance.journal.post", Role::Member),
            ("finance.journal.post", Role::ReadOnly),
            ("finance.journal.post", Role::Sales),
            ("finance.account.manage", Role::Member),
            ("finance.account.manage", Role::ReadOnly),
            ("finance.account.manage", Role::Sales),
            ("finance.account.manage", Role::Manager),
            ("finance.period.close", Role::Member),
            ("finance.period.close", Role::ReadOnly),
            ("finance.period.close", Role::Sales),
            ("finance.period.close", Role::Manager),
            ("finance.period.reopen", Role::Member),
            ("finance.period.reopen", Role::ReadOnly),
            ("finance.period.reopen", Role::Sales),
            ("finance.period.reopen", Role::Manager),
            ("finance.bank.reconcile", Role::Member),
            ("finance.bank.reconcile", Role::ReadOnly),
            ("finance.bank.reconcile", Role::Sales),
            ("finance.bank.reconcile", Role::Manager),
            ("finance.expense_policy.manage", Role::Member),
            ("finance.expense_policy.manage", Role::ReadOnly),
            ("finance.expense_policy.manage", Role::Sales),
            ("finance.expense_policy.manage", Role::Manager),
            ("finance.reimbursement.manage", Role::Member),
            ("finance.reimbursement.manage", Role::ReadOnly),
            ("finance.reimbursement.manage", Role::Sales),
            ("finance.reimbursement.manage", Role::Manager),
            // Phase 2.5 inventory writes
            ("inventory.item.write", Role::Member),
            ("inventory.item.write", Role::ReadOnly),
            ("inventory.item.write", Role::Sales),
            ("inventory.warehouse.write", Role::Member),
            ("inventory.warehouse.write", Role::ReadOnly),
            ("inventory.warehouse.write", Role::Sales),
            ("inventory.stock.move", Role::Member),
            ("inventory.stock.move", Role::ReadOnly),
            ("inventory.stock.move", Role::Sales),
            ("inventory.supplier.write", Role::Member),
            ("inventory.supplier.write", Role::ReadOnly),
            ("inventory.supplier.write", Role::Sales),
            ("inventory.purchase_order.write", Role::Member),
            ("inventory.purchase_order.write", Role::ReadOnly),
            ("inventory.purchase_order.write", Role::Sales),
            ("inventory.goods_receipt.write", Role::Member),
            ("inventory.goods_receipt.write", Role::ReadOnly),
            ("inventory.goods_receipt.write", Role::Sales),
            ("inventory.asset.write", Role::Member),
            ("inventory.asset.write", Role::ReadOnly),
            ("inventory.asset.write", Role::Sales),
            ("analytics.schedule.write", Role::Member),
            ("analytics.schedule.write", Role::ReadOnly),
            ("analytics.schedule.write", Role::Sales),
            ("analytics.schedule.write", Role::Finance),
            ("analytics.schedule.write", Role::Manager),
            ("admin.marketplace.write", Role::Member),
            ("admin.marketplace.write", Role::ReadOnly),
            ("admin.marketplace.write", Role::Sales),
            ("admin.marketplace.write", Role::Finance),
            ("admin.marketplace.write", Role::Manager),
            ("admin.marketplace.review", Role::Member),
            ("admin.marketplace.review", Role::ReadOnly),
            ("admin.marketplace.review", Role::Sales),
            ("admin.marketplace.review", Role::Finance),
            ("admin.marketplace.review", Role::Manager),
            ("admin.marketplace.install", Role::Member),
            ("admin.marketplace.install", Role::ReadOnly),
            ("admin.marketplace.install", Role::Sales),
            ("admin.marketplace.install", Role::Finance),
            ("admin.marketplace.uninstall", Role::Member),
            ("admin.marketplace.uninstall", Role::ReadOnly),
            ("admin.marketplace.uninstall", Role::Sales),
            ("admin.marketplace.uninstall", Role::Finance),
        ];
        for (perm, role) in deny_pairs {
            let p = Principal::with_roles(vec![*role]);
            assert!(
                !is_allowed(&p, &PermissionId::from(*perm)),
                "{role:?} must be denied {perm}"
            );
        }
        // Owner allowed all sensitive
        let owner = Principal::with_roles(vec![Role::Owner]);
        for perm in SENSITIVE_ACTIONS {
            assert!(
                is_allowed(&owner, &PermissionId::from(*perm)),
                "owner must allow {perm}"
            );
        }
        // Finance can issue/approve invoices and manage payments/expenses
        let finance = Principal::with_roles(vec![Role::Finance]);
        assert!(is_allowed(&finance, &perms::finance_invoice_approve()));
        assert!(is_allowed(&finance, &perms::finance_invoice_issue()));
        assert!(is_allowed(&finance, &perms::finance_payment_create()));
        assert!(is_allowed(&finance, &perms::finance_expense_approve()));
        // Phase 2.1: Finance can read HR sensitive fields (compensation/IDs)
        assert!(is_allowed(&finance, &perms::hr_employee_read_sensitive()));
        assert!(!is_allowed(&finance, &perms::hr_employee_write()));
        assert!(!is_allowed(&finance, &perms::hr_employee_offboard()));
        // Phase 2.3: Finance can read/approve payroll and post journals; cannot draft/run
        assert!(is_allowed(&finance, &perms::hr_payroll_read()));
        assert!(is_allowed(&finance, &perms::hr_payroll_approve()));
        assert!(is_allowed(&finance, &perms::finance_journal_post()));
        assert!(is_allowed(&finance, &perms::finance_account_manage()));
        assert!(is_allowed(&finance, &perms::finance_period_close()));
        assert!(is_allowed(&finance, &perms::finance_period_reopen()));
        assert!(is_allowed(&finance, &perms::finance_bank_reconcile()));
        assert!(!is_allowed(&finance, &perms::hr_payroll_write()));
        assert!(!is_allowed(&finance, &perms::hr_payroll_run()));
        // Manager can run People write/onboard/offboard + payroll
        let manager = Principal::with_roles(vec![Role::Manager]);
        assert!(is_allowed(&manager, &perms::hr_employee_write()));
        assert!(is_allowed(&manager, &perms::hr_employee_onboard()));
        assert!(is_allowed(&manager, &perms::hr_employee_offboard()));
        assert!(is_allowed(&manager, &perms::hr_employee_read_sensitive()));
        assert!(is_allowed(&manager, &perms::hr_attendance_write()));
        assert!(is_allowed(&manager, &perms::hr_leave_write()));
        assert!(is_allowed(&manager, &perms::hr_leave_approve()));
        assert!(is_allowed(&manager, &perms::hr_payroll_read()));
        assert!(is_allowed(&manager, &perms::hr_payroll_write()));
        assert!(is_allowed(&manager, &perms::hr_payroll_approve()));
        assert!(is_allowed(&manager, &perms::hr_payroll_run()));
        assert!(is_allowed(&manager, &perms::finance_journal_post()));
        // Member cannot issue invoices or read HR sensitive / others' payslips
        let member = Principal::with_roles(vec![Role::Member]);
        assert!(!is_allowed(&member, &perms::finance_invoice_issue()));
        assert!(!is_allowed(&member, &perms::hr_employee_read_sensitive()));
        assert!(!is_allowed(&member, &perms::hr_employee_write()));
        assert!(is_allowed(&member, &perms::hr_leave_write()));
        assert!(is_allowed(&member, &perms::hr_attendance_write()));
        assert!(!is_allowed(&member, &perms::hr_leave_approve()));
        assert!(!is_allowed(&member, &perms::hr_payroll_read()));
        assert!(!is_allowed(&member, &perms::hr_payroll_write()));
        assert!(!is_allowed(&member, &perms::hr_payroll_approve()));
        assert!(!is_allowed(&member, &perms::hr_payroll_run()));
        assert!(!is_allowed(&member, &perms::finance_journal_post()));
        // Admin can invite / assign / revoke / settings
        let admin = Principal::with_roles(vec![Role::Admin]);
        assert!(is_allowed(&admin, &perms::workspace_member_invite()));
        assert!(is_allowed(&admin, &perms::workspace_role_assign()));
        assert!(is_allowed(&admin, &perms::workspace_member_revoke()));
        assert!(is_allowed(&admin, &perms::workspace_org_update_settings()));
        assert!(is_allowed(&admin, &perms::finance_invoice_issue()));
    }

    #[test]
    fn scope_covers_hierarchy() {
        assert!(Scope::Organization.covers(Scope::Own));
        assert!(Scope::Team.covers(Scope::Own));
        assert!(!Scope::Own.covers(Scope::Team));
    }

    #[test]
    fn permission_cache_ttl_and_version() {
        let cache = PermissionSetCache::with_ttl(Duration::from_millis(50));
        let mut set = HashSet::new();
        set.insert("workspace.dashboard.read".into());
        cache.put("mem1", 1, set.clone());
        assert_eq!(cache.get("mem1", 1), Some(set));
        assert!(cache.get("mem1", 2).is_none());
        cache.invalidate_membership("mem1");
        assert!(cache.get("mem1", 1).is_none());
        cache.put("mem1", 3, HashSet::from(["x".into()]));
        std::thread::sleep(Duration::from_millis(60));
        assert!(cache.get("mem1", 3).is_none());
    }

    #[test]
    fn read_only_denied_sensitive() {
        let p = Principal::with_roles(vec![Role::ReadOnly]);
        for perm in SENSITIVE_ACTIONS {
            assert!(!is_allowed(&p, &PermissionId::from(*perm)));
        }
    }

    #[test]
    fn abac_time_window_denies_outside_hours() {
        use chrono::{TimeZone, Utc};
        let mut p = Principal::with_roles(vec![]);
        p.statements.push(
            Statement::allow(perms::hr_payroll_read()).with_conditions(vec![
                AbacCondition::TimeWindow {
                    start: "09:00:00".into(),
                    end: "17:00:00".into(),
                    weekdays: vec![1, 2, 3, 4, 5],
                },
            ]),
        );
        let inside = EvaluationContext::at(Utc.with_ymd_and_hms(2026, 3, 2, 10, 0, 0).unwrap());
        let outside = EvaluationContext::at(Utc.with_ymd_and_hms(2026, 3, 2, 19, 0, 0).unwrap());
        assert!(is_allowed_with_context(
            &p,
            &perms::hr_payroll_read(),
            Scope::Organization,
            &inside
        ));
        let d = decide_with_context(&p, &perms::hr_payroll_read(), Scope::Organization, &outside);
        assert_eq!(d.decision, Decision::Deny);
        assert_eq!(d.reason, "abac_deny");
    }

    #[test]
    fn abac_record_state_denies_closed_or_locked() {
        let mut p = Principal::with_roles(vec![]);
        p.statements.push(
            Statement::allow(perms::finance_period_close()).with_conditions(vec![
                AbacCondition::RecordState {
                    allow: vec!["open".into()],
                    deny: vec!["closed".into(), "locked".into()],
                },
            ]),
        );
        assert!(is_allowed_with_context(
            &p,
            &perms::finance_period_close(),
            Scope::Organization,
            &EvaluationContext::default().with_record_state("open"),
        ));
        assert!(!is_allowed_with_context(
            &p,
            &perms::finance_period_close(),
            Scope::Organization,
            &EvaluationContext::default().with_record_state("closed"),
        ));
        assert!(!is_allowed_with_context(
            &p,
            &perms::finance_period_close(),
            Scope::Organization,
            &EvaluationContext::default().with_record_state("locked"),
        ));
    }

    #[test]
    fn field_level_member_finance_matrix() {
        let finance = Principal::with_roles(vec![Role::Finance]);
        let member = Principal::with_roles(vec![Role::Member]);
        assert!(is_allowed(&finance, &perms::hr_field_compensation_read()));
        assert!(is_allowed(&finance, &perms::hr_field_bank_read()));
        assert!(is_allowed(
            &finance,
            &perms::finance_field_bank_account_read()
        ));
        assert!(!is_allowed(&member, &perms::hr_field_compensation_read()));
        assert!(!is_allowed(&member, &perms::hr_field_bank_read()));
        assert!(!is_allowed(
            &member,
            &perms::finance_field_bank_account_read()
        ));
        assert!(!is_allowed(&member, &perms::hr_employee_read_sensitive()));
    }
}
