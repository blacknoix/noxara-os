//! Phase 1.2 Workspace bounded context.
//!
//! Owns: organizations, memberships, teams, departments, roles, permissions,
//! invitations, org settings, OrgProvisioning.

mod catalogue_sync;
pub mod last_owner;
pub mod principal;
pub mod provisioning;
pub mod types;

pub mod handlers;

pub use catalogue_sync::assert_matches_catalogue;
pub use handlers::router;
pub use principal::load_principal;

use companyos_errors::{AppError, ErrorCode};
use sqlx::PgPool;

use crate::auth::extract::AuthUser;
use crate::state::AppState;

/// Sync permission catalogue into DB (idempotent). Call after migrate.
pub async fn sync_permission_catalogue(pool: &PgPool) -> anyhow::Result<()> {
    catalogue_sync::sync(pool).await
}

pub(crate) fn require_perm(
    user: &AuthUser,
    state: &AppState,
    permission: &companyos_authz::PermissionId,
) -> Result<companyos_authz::Principal, AppError> {
    let request_id = user.ctx.request_id.clone();
    let principal = companyos_authz::Principal::with_roles(
        user.roles
            .iter()
            .filter_map(|r| companyos_authz::Role::parse(r))
            .collect(),
    );
    if !companyos_authz::is_allowed(&principal, permission) {
        state
            .perm_cache
            .invalidate_membership(&user.membership_id.to_string());
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!("missing permission {}", permission.as_str()),
        ));
    }
    Ok(principal)
}

pub(crate) async fn audit_mutation(
    pool: &PgPool,
    org_id: uuid::Uuid,
    actor: uuid::Uuid,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    metadata: serde_json::Value,
) {
    let _ = sqlx::query(
        r#"
        INSERT INTO audit_entry (
            id, org_id, actor_user_id, actor_on_behalf_of, actor_is_ai,
            action, resource_type, resource_id, metadata
        ) VALUES ($1,$2,$3,$3,false,$4,$5,$6,$7)
        "#,
    )
    .bind(companyos_ids::new_uuid_v7())
    .bind(org_id)
    .bind(actor)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(metadata)
    .execute(pool)
    .await;
}
