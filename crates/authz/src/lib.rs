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

use std::collections::HashSet;

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

/// Built-in Phase 0 permissions.
pub mod perms {
    use super::PermissionId;

    pub fn workspace_dashboard_read() -> PermissionId {
        PermissionId::new("workspace", "dashboard", "read")
    }

    pub fn finance_invoice_approve() -> PermissionId {
        PermissionId::new("finance", "invoice", "approve")
    }
}

/// Mini Phase 0 roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Member,
}

impl Role {
    /// Default allows for the role (explicit denials still win).
    pub fn default_allows(self) -> HashSet<PermissionId> {
        match self {
            Self::Owner => HashSet::from([
                perms::workspace_dashboard_read(),
                perms::finance_invoice_approve(),
            ]),
            Self::Member => HashSet::from([perms::workspace_dashboard_read()]),
        }
    }
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
/// 2. Explicit allow in `principal.statements` → Allow  
/// 3. Allow from any role default → Allow  
/// 4. Otherwise → Deny
pub fn decide(principal: &Principal, permission: &PermissionId) -> DecisionDetail {
    // 1. Explicit deny wins.
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

    // 2. Explicit allow.
    if principal
        .statements
        .iter()
        .any(|s| s.effect == Effect::Allow && s.permission == *permission)
    {
        return DecisionDetail {
            decision: Decision::Allow,
            permission: permission.clone(),
            reason: "explicit_allow",
        };
    }

    // 3. Role defaults.
    for role in &principal.roles {
        if role.default_allows().contains(permission) {
            return DecisionDetail {
                decision: Decision::Allow,
                permission: permission.clone(),
                reason: "role_allow",
            };
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
        let unknown = PermissionId::from("sales.deal.delete");
        assert!(!is_allowed(&p, &unknown));
    }

    #[test]
    fn explicit_deny_wins_over_owner_role() {
        let mut p = Principal::with_roles(vec![Role::Owner]);
        p.statements.push(Statement {
            effect: Effect::Deny,
            permission: perms::finance_invoice_approve(),
        });
        let d = decide(&p, &perms::finance_invoice_approve());
        assert_eq!(d.decision, Decision::Deny);
        assert_eq!(d.reason, "explicit_deny");
    }

    #[test]
    fn explicit_deny_wins_over_explicit_allow() {
        let mut p = Principal::with_roles(vec![]);
        p.statements.push(Statement {
            effect: Effect::Allow,
            permission: perms::workspace_dashboard_read(),
        });
        p.statements.push(Statement {
            effect: Effect::Deny,
            permission: perms::workspace_dashboard_read(),
        });
        assert!(!is_allowed(&p, &perms::workspace_dashboard_read()));
    }

    #[test]
    fn explicit_allow_for_member_invoice() {
        let mut p = Principal::with_roles(vec![Role::Member]);
        assert!(!is_allowed(&p, &perms::finance_invoice_approve()));
        p.statements.push(Statement {
            effect: Effect::Allow,
            permission: perms::finance_invoice_approve(),
        });
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
        // explicit deny
        let mut p = Principal::with_roles(vec![Role::Owner]);
        p.statements.push(Statement {
            effect: Effect::Deny,
            permission: perms::workspace_dashboard_read(),
        });
        assert_eq!(
            decide(&p, &perms::workspace_dashboard_read()).reason,
            "explicit_deny"
        );

        // role allow
        let p = Principal::with_roles(vec![Role::Member]);
        assert_eq!(
            decide(&p, &perms::workspace_dashboard_read()).reason,
            "role_allow"
        );
    }
}
