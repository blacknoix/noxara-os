//! Compile an authz [`Scope`] into a SQL predicate appended to a
//! [`sqlx::QueryBuilder`] — list endpoints filter in SQL, never via
//! post-fetch pagination.

use companyos_authz::{Effect, PermissionId, Principal, Scope};
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

fn scope_rank(s: Scope) -> u8 {
    match s {
        Scope::Own => 0,
        Scope::Team => 1,
        Scope::Department => 2,
        Scope::Organization => 3,
    }
}

/// Widest scope for which the principal holds an `Allow` for `permission`
/// (explicit statement, or a role default at organization scope). Callers
/// must already have confirmed the permission is not explicitly denied via
/// [`crate::principal::enforce`] before calling this.
pub fn scope_for_permission(principal: &Principal, permission: &PermissionId) -> Scope {
    let mut best: Option<Scope> = None;
    for st in &principal.statements {
        if st.effect == Effect::Allow && st.permission == *permission {
            best = Some(match best {
                Some(b) if scope_rank(b) >= scope_rank(st.scope) => b,
                _ => st.scope,
            });
        }
    }
    if best.is_none() {
        for role in &principal.roles {
            if role.default_allows().contains(permission) {
                best = Some(Scope::Organization);
            }
        }
    }
    best.unwrap_or(Scope::Own)
}

/// Push ` AND (...)` onto `qb` restricting rows to `owner_user_id` visible at
/// `scope`. Assumes the target table has an `owner_user_id UUID` column.
///
/// - `Organization`: no filter (already tenant-scoped by RLS + org_id).
/// - `Own`: `owner_user_id = actor`.
/// - `Team`: `owner_user_id = actor OR owner_user_id IN (SELECT user_id FROM
///   membership WHERE org_id = org AND team_id = team AND revoked_at IS NULL)`.
/// - `Department`: same shape keyed by `department_id`.
pub fn push_owner_predicate(
    qb: &mut QueryBuilder<'_, Postgres>,
    scope: Scope,
    org_id: Uuid,
    actor_user_id: Uuid,
    team_id: Option<Uuid>,
    department_id: Option<Uuid>,
) {
    match scope {
        Scope::Organization => {}
        Scope::Own => {
            qb.push(" AND owner_user_id = ");
            qb.push_bind(actor_user_id);
        }
        Scope::Team => {
            qb.push(" AND (owner_user_id = ");
            qb.push_bind(actor_user_id);
            if let Some(team) = team_id {
                qb.push(" OR owner_user_id IN (SELECT user_id FROM membership WHERE org_id = ");
                qb.push_bind(org_id);
                qb.push(" AND team_id = ");
                qb.push_bind(team);
                qb.push(" AND revoked_at IS NULL)");
            }
            qb.push(")");
        }
        Scope::Department => {
            qb.push(" AND (owner_user_id = ");
            qb.push_bind(actor_user_id);
            if let Some(dept) = department_id {
                qb.push(
                    " OR owner_user_id IN (SELECT user_id FROM membership WHERE org_id = ",
                );
                qb.push_bind(org_id);
                qb.push(" AND department_id = ");
                qb.push_bind(dept);
                qb.push(" AND revoked_at IS NULL)");
            }
            qb.push(")");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_authz::{perms, Role, Statement};

    #[test]
    fn organization_scope_from_role_default() {
        let p = Principal::with_roles(vec![Role::Sales]);
        let scope = scope_for_permission(&p, &perms::sales_deal_update());
        assert_eq!(scope, Scope::Organization);
    }

    #[test]
    fn explicit_own_statement_overrides_when_narrower_than_default() {
        let mut p = Principal::with_roles(vec![]);
        p.statements.push(Statement {
            effect: Effect::Allow,
            permission: perms::sales_deal_update(),
            scope: Scope::Own,
        });
        let scope = scope_for_permission(&p, &perms::sales_deal_update());
        assert_eq!(scope, Scope::Own);
    }

    #[test]
    fn widest_explicit_statement_wins() {
        let mut p = Principal::with_roles(vec![]);
        p.statements.push(Statement {
            effect: Effect::Allow,
            permission: perms::sales_deal_update(),
            scope: Scope::Own,
        });
        p.statements.push(Statement {
            effect: Effect::Allow,
            permission: perms::sales_deal_update(),
            scope: Scope::Team,
        });
        let scope = scope_for_permission(&p, &perms::sales_deal_update());
        assert_eq!(scope, Scope::Team);
    }
}
