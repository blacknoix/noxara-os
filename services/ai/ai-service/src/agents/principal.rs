//! Machine principal for agents.
//!
//! Documented machine principal with `on_behalf_of` a human (or a scheduled
//! policy). Effective permissions = policy allow-list ∩ on_behalf_of ∩ org roles.
//! Narrower wins. No superuser / cross-tenant AI service account.

use companyos_authz::{PermissionId, Principal};

use super::policy::AgentPolicyDoc;

/// Intersect the human (or scheduled) principal with the policy allow-list.
pub fn effective_principal(on_behalf_of: &Principal, policy: &AgentPolicyDoc) -> Principal {
    on_behalf_of.intersect_scopes(&policy.allowed_permissions)
}

/// Whether the effective principal may use `perm` under the policy.
pub fn effective_allows(effective: &Principal, policy: &AgentPolicyDoc, perm: &str) -> bool {
    if !super::policy::policy_allows_permission(policy, perm) {
        return false;
    }
    companyos_authz::is_allowed(effective, &PermissionId::from(perm))
}
