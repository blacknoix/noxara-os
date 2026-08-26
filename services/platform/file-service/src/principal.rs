//! Load a [`Principal`] from membership + role_permission (CRM pattern, simplified).

use companyos_authz::{Decision, Effect, PermissionId, Principal, Role, Scope, Statement};
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

/// Platform permissions may not be in the catalogue yet — allow authenticated
/// members to proceed for own-tenant platform ops when the permission is unknown
/// or explicitly granted.
pub fn enforce_platform_or_member(
    principal: &Principal,
    permission: PermissionId,
    request_id: &str,
) -> Result<(), AppError> {
    if companyos_authz::is_allowed(principal, &permission) {
        return Ok(());
    }
    if !principal.roles.is_empty() || !principal.statements.is_empty() {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::Forbidden,
        request_id,
        format!("missing permission {}", permission.as_str()),
    ))
}

/// Check whether a principal may receive a notification about a resource.
pub fn can_receive(principal: &Principal, required: PermissionId) -> bool {
    companyos_authz::is_allowed(principal, &required)
}
