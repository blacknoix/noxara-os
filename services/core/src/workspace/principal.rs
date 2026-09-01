#![allow(clippy::type_complexity)]
//! Load a [`Principal`] from membership + org_role + role_permission
//! + inherited team grants + active permission delegations.

use companyos_authz::{Effect, PermissionId, Principal, Role, Scope, Statement};
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::OrgId;
use uuid::Uuid;

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

    let mem: Option<(Uuid, String, i64, Option<Uuid>, String, Option<Uuid>)> = sqlx::query_as(
        r#"
        SELECT id, role, policy_version, role_id, status, team_id
        FROM membership
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some((membership_id, role_key, policy_version, role_id, status, team_id)) = mem else {
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
        let rows: Vec<(String, String, String, serde_json::Value)> = sqlx::query_as(
            r#"
            SELECT permission_id, effect, scope, conditions
            FROM role_permission
            WHERE role_id = $1 AND org_id = $2
            "#,
        )
        .bind(rid)
        .bind(org_id.as_uuid())
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

        for (perm, effect, scope, conditions_json) in rows {
            let effect = match effect.as_str() {
                "deny" => Effect::Deny,
                _ => Effect::Allow,
            };
            let scope = Scope::parse(&scope).unwrap_or(Scope::Organization);
            let conditions: Vec<companyos_authz::AbacCondition> =
                serde_json::from_value(conditions_json).unwrap_or_default();
            statements.push(Statement {
                effect,
                permission: PermissionId::from(perm.as_str()),
                scope,
                conditions,
            });
        }
    }

    // Inherited grants: walk parent_team_id chain from membership.team_id.
    if let Some(start_team) = team_id {
        let mut cursor = Some(start_team);
        let mut visited = std::collections::HashSet::new();
        while let Some(tid) = cursor {
            if !visited.insert(tid) {
                break;
            }
            let grants: Vec<(String, String, String)> = sqlx::query_as(
                r#"
                SELECT permission_id, effect, scope
                FROM permission_inherit_grant
                WHERE org_id = $1 AND team_id = $2
                "#,
            )
            .bind(org_id.as_uuid())
            .bind(tid)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            for (perm, effect, scope) in grants {
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
            let parent: Option<Uuid> =
                sqlx::query_scalar("SELECT parent_team_id FROM team WHERE id = $1 AND org_id = $2")
                    .bind(tid)
                    .bind(org_id.as_uuid())
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?
                    .flatten();
            cursor = parent;
        }
    }

    // Active, non-expired delegations TO this membership.
    let delegations: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT permission_id, scope
        FROM permission_delegation
        WHERE org_id = $1
          AND to_membership_id = $2
          AND revoked_at IS NULL
          AND expires_at > now()
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(membership_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    for (perm, scope) in delegations {
        let scope = Scope::parse(&scope).unwrap_or(Scope::Organization);
        statements.push(Statement {
            effect: Effect::Allow,
            permission: PermissionId::from(perm.as_str()),
            scope,
            conditions: vec![],
        });
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
