//! Load a [`Principal`] from `membership` + `role_permission` (CRM pattern).

use companyos_authz::{Decision, Effect, PermissionId, Principal, Role, Scope, Statement};
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::OrgId;
use uuid::Uuid;

use crate::marketplace::auth::AuthCtx;
use crate::AppState;

pub async fn load_principal(
    pool: &sqlx::PgPool,
    org_id: OrgId,
    user_id: Uuid,
    request_id: &str,
) -> Result<(Principal, i64, Uuid), AppError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    companyos_tenancy::set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    #[allow(clippy::type_complexity)]
    let mem: Option<(Uuid, String, i64, Option<Uuid>, String)> = sqlx::query_as(
        r#"
        SELECT id, role, policy_version, role_id, status
        FROM membership
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some((membership_id, role_key, policy_version, role_id, status)) = mem else {
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

    Ok((
        Principal { roles, statements },
        policy_version,
        membership_id,
    ))
}

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

/// Resolve the caller's principal and enforce `permission`.
///
/// LOCAL-ONLY dev headers skip the membership lookup and derive the principal
/// from the header role so local runs can still exercise role boundaries.
pub async fn authorize(
    state: &AppState,
    auth: &AuthCtx,
    permission: PermissionId,
) -> Result<Principal, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let principal = if auth.local_bypass {
        let roles = auth
            .roles
            .iter()
            .filter_map(|r| Role::parse(r))
            .collect::<Vec<_>>();
        Principal::with_roles(if roles.is_empty() {
            vec![Role::Owner]
        } else {
            roles
        })
    } else {
        load_principal(
            &state.pool,
            auth.ctx.org_id,
            auth.ctx.actor.on_behalf_of,
            request_id,
        )
        .await?
        .0
    };
    enforce(&principal, permission, request_id)?;
    Ok(principal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_authz::perms;

    #[test]
    fn member_may_read_but_not_review_or_publish() {
        let member = Principal::with_roles(vec![Role::Member]);
        assert!(enforce(&member, perms::admin_marketplace_read(), "t").is_ok());
        assert!(enforce(&member, perms::admin_marketplace_review(), "t").is_err());
        assert!(enforce(&member, perms::admin_marketplace_write(), "t").is_err());
        assert!(enforce(&member, perms::admin_marketplace_install(), "t").is_err());
        assert!(enforce(&member, perms::admin_marketplace_uninstall(), "t").is_err());
    }

    #[test]
    fn owner_and_admin_hold_every_marketplace_permission() {
        for role in [Role::Owner, Role::Admin] {
            let principal = Principal::with_roles(vec![role]);
            for permission in [
                perms::admin_marketplace_read(),
                perms::admin_marketplace_write(),
                perms::admin_marketplace_review(),
                perms::admin_marketplace_install(),
                perms::admin_marketplace_uninstall(),
            ] {
                assert!(
                    enforce(&principal, permission.clone(), "t").is_ok(),
                    "{role:?} missing {}",
                    permission.as_str()
                );
            }
        }
    }
}
