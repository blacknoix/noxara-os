//! Load a [`Principal`] (+ membership scope) from `membership` + `role_permission`.
//!
//! Mirrors `services/core/src/workspace/principal.rs` — `companyos_authz` is
//! the sole PDP; this module only assembles the inputs it needs.

use companyos_authz::{Decision, Effect, PermissionId, Principal, Role, Scope, Statement};
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::OrgId;
use uuid::Uuid;

/// Membership scope context needed for scoped list/detail queries.
#[derive(Debug, Clone)]
pub struct MembershipScope {
    pub principal: Principal,
    pub policy_version: i64,
    pub membership_id: Uuid,
    pub team_id: Option<Uuid>,
    pub department_id: Option<Uuid>,
}

/// Load `(Principal, policy_version, membership_id)` — compatible shape with
/// core's `load_principal` for callers that don't need team/department.
pub async fn load_principal(
    pool: &sqlx::PgPool,
    org_id: OrgId,
    user_id: Uuid,
    request_id: &str,
) -> Result<(Principal, i64, Uuid), AppError> {
    let scope = load_membership_scope(pool, org_id, user_id, request_id).await?;
    Ok((scope.principal, scope.policy_version, scope.membership_id))
}

/// Load the full membership scope: principal + policy_version + membership_id
/// + team_id/department_id (used to compile authz scope into SQL predicates).
pub async fn load_membership_scope(
    pool: &sqlx::PgPool,
    org_id: OrgId,
    user_id: Uuid,
    request_id: &str,
) -> Result<MembershipScope, AppError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    companyos_tenancy::set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    #[allow(clippy::type_complexity)]
    let mem: Option<(
        Uuid,
        String,
        i64,
        Option<Uuid>,
        String,
        Option<Uuid>,
        Option<Uuid>,
    )> = sqlx::query_as(
        r#"
        SELECT id, role, policy_version, role_id, status, team_id, department_id
        FROM membership
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some((membership_id, role_key, policy_version, role_id, status, team_id, department_id)) =
        mem
    else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "membership not found",
        ));
    };
    if status != "active" {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "membership not active",
        ));
    }

    let mut roles = Vec::new();
    if let Some(r) = Role::parse(&role_key) {
        roles.push(r);
    }

    let mut statements = Vec::new();
    if let Some(rid) = role_id {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT permission_id, effect, scope
            FROM role_permission
            WHERE role_id = $1 AND org_id = $2
            "#,
        )
        .bind(rid)
        .bind(org_id.as_uuid())
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

        for (perm, effect, scope) in rows {
            let effect = match effect.as_str() {
                "deny" => Effect::Deny,
                _ => Effect::Allow,
            };
            let scope = Scope::parse(&scope).unwrap_or(Scope::Organization);
            statements.push(Statement {
                effect,
                permission: PermissionId::from(perm.as_str()),
                scope,
                conditions: vec![],
            });
        }
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(MembershipScope {
        principal: Principal { roles, statements },
        policy_version,
        membership_id,
        team_id,
        department_id,
    })
}

/// Load membership scope and, for API-key callers, restrict the principal to
/// the key's effective scopes (already intersected with the owner role).
pub async fn load_membership_scope_for(
    pool: &sqlx::PgPool,
    auth: &crate::auth::AuthCtx,
    request_id: &str,
) -> Result<MembershipScope, AppError> {
    let mut scope =
        load_membership_scope(pool, auth.ctx.org_id, auth.ctx.actor.user_id, request_id).await?;
    if let Some(ref scopes) = auth.api_key_scopes {
        scope.principal = Principal::from_permission_ids(scopes.iter().map(|s| s.as_str()));
    }
    Ok(scope)
}

/// Enforce an organization-scoped permission (no resource-level scope check).
pub fn enforce(
    principal: &Principal,
    permission: PermissionId,
    request_id: &str,
) -> Result<(), AppError> {
    let decision = companyos_authz::decide(principal, &permission);
    if decision.decision != Decision::Allow {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!("missing permission {}", permission.as_str()),
        ));
    }
    Ok(())
}

/// Enforce a permission against a specific required resource scope (e.g. a
/// single row's ownership relative to the caller) using `decide_with_scope`.
pub fn enforce_scoped(
    principal: &Principal,
    permission: PermissionId,
    required_scope: Scope,
    request_id: &str,
) -> Result<(), AppError> {
    let decision = companyos_authz::decide_with_scope(principal, &permission, required_scope);
    if decision.decision != Decision::Allow {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!(
                "missing permission {} at scope {}",
                permission.as_str(),
                required_scope.as_str()
            ),
        ));
    }
    Ok(())
}

/// Coarse gate: "is this permission allowed at all, for at least the
/// caller's own resources?" Used before list/detail queries; the finer
/// per-row check (for detail endpoints) happens after the row is fetched via
/// [`required_scope_for_owner_row`] + a second [`enforce_scoped`] call.
pub fn enforce_any_scope(
    principal: &Principal,
    permission: PermissionId,
    request_id: &str,
) -> Result<(), AppError> {
    enforce_scoped(principal, permission, Scope::Own, request_id)
}

/// Look up a membership's `(team_id, department_id)` by user id (defaults to
/// `(None, None)` when no active membership row exists).
pub async fn owner_membership_scope(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    owner_user_id: Uuid,
) -> Result<(Option<Uuid>, Option<Uuid>), sqlx::Error> {
    let row: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        r#"
        SELECT team_id, department_id FROM membership
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(owner_user_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.unwrap_or((None, None)))
}

/// Async variant of [`required_scope_for_owner`] that resolves the owner's
/// team/department via `membership` before comparing.
pub async fn required_scope_for_owner_row(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    actor_user_id: Uuid,
    actor_team_id: Option<Uuid>,
    actor_department_id: Option<Uuid>,
    owner_user_id: Option<Uuid>,
) -> Result<Scope, sqlx::Error> {
    let Some(owner) = owner_user_id else {
        return Ok(Scope::Organization);
    };
    if owner == actor_user_id {
        return Ok(Scope::Own);
    }
    let (owner_team, owner_department) = owner_membership_scope(conn, org_id, owner).await?;
    Ok(required_scope_for_owner(
        actor_user_id,
        Some(owner),
        actor_team_id,
        owner_team,
        actor_department_id,
        owner_department,
    ))
}

/// Determine the required scope for a resource row given its owner, relative
/// to the caller's membership (own < team < department < organization).
pub fn required_scope_for_owner(
    actor_user_id: Uuid,
    owner_user_id: Option<Uuid>,
    actor_team_id: Option<Uuid>,
    owner_team_id: Option<Uuid>,
    actor_department_id: Option<Uuid>,
    owner_department_id: Option<Uuid>,
) -> Scope {
    match owner_user_id {
        Some(owner) if owner == actor_user_id => Scope::Own,
        _ => {
            if let (Some(a), Some(o)) = (actor_team_id, owner_team_id) {
                if a == o {
                    return Scope::Team;
                }
            }
            if let (Some(a), Some(o)) = (actor_department_id, owner_department_id) {
                if a == o {
                    return Scope::Department;
                }
            }
            Scope::Organization
        }
    }
}
