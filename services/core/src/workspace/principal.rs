//! Load a [`Principal`] from membership + org_role + role_permission.

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
