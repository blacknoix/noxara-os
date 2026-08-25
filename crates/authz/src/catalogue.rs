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

/// Full Phase 1.2 permission catalogue (Workspace + reserved Finance/Admin stubs).
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
        id: "finance.invoice.approve",
        context: "finance",
        resource: "invoice",
        action: "approve",
        description: "Approve invoices (catalogue reserved for Finance)",
        sensitive: true,
    },
];

/// Sensitive permission IDs used by deny-matrix DoD tests.
pub const SENSITIVE_ACTIONS: &[&str] = &[
    "workspace.member.invite",
    "workspace.role.assign",
    "workspace.member.revoke",
    "workspace.org.update_settings",
    "finance.invoice.approve",
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
    pub fn admin_membership_manage() -> PermissionId {
        PermissionId::from("admin.membership.manage")
    }
    pub fn admin_sso_manage() -> PermissionId {
        PermissionId::from("admin.sso.manage")
    }
    pub fn finance_invoice_approve() -> PermissionId {
        PermissionId::from("finance.invoice.approve")
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
        | "workspace.department.read" => Scope::Organization,
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
