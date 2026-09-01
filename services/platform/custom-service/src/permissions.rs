//! Register dynamic `custom.{slug}.read|write` permissions on publish / package import.
//! Sole PDP remains `crates/authz`; these rows feed role grants + deny-by-default.

use companyos_authz::{is_dynamic_custom_entity_permission, perms};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::OrgId;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Ensure permission_definition rows exist for a published slug and grant Owner/Admin.
pub async fn register_entity_permissions(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    slug: &str,
) -> Result<(String, String), sqlx::Error> {
    let read_id = perms::custom_entity_read(slug).0;
    let write_id = perms::custom_entity_write(slug).0;
    assert!(is_dynamic_custom_entity_permission(&read_id));
    assert!(is_dynamic_custom_entity_permission(&write_id));

    for (id, action, sensitive) in [
        (read_id.as_str(), "read", false),
        (write_id.as_str(), "write", true),
    ] {
        sqlx::query(
            r#"
            INSERT INTO permission_definition (id, context, resource, action, description, sensitive)
            VALUES ($1, 'custom', $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET
                description = EXCLUDED.description,
                sensitive = EXCLUDED.sensitive
            "#,
        )
        .bind(id)
        .bind(slug)
        .bind(action)
        .bind(format!("Custom entity {slug} {action}"))
        .bind(sensitive)
        .execute(&mut **tx)
        .await?;
    }

    // Grant to Owner and Admin system roles for this org.
    let roles: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, system_key FROM org_role
        WHERE org_id = $1 AND system_key IN ('owner', 'admin') AND is_system = true
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    for (role_id, _) in &roles {
        for perm in [&read_id, &write_id] {
            let exists: Option<(Uuid,)> = sqlx::query_as(
                r#"
                SELECT id FROM role_permission
                WHERE role_id = $1 AND permission_id = $2 AND org_id = $3
                "#,
            )
            .bind(role_id)
            .bind(perm)
            .bind(org_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;
            if exists.is_some() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO role_permission (id, org_id, role_id, permission_id, effect, scope)
                VALUES ($1, $2, $3, $4, 'allow', 'organization')
                "#,
            )
            .bind(new_uuid_v7())
            .bind(org_id.as_uuid())
            .bind(role_id)
            .bind(perm)
            .execute(&mut **tx)
            .await?;
        }
    }

    // Bump policy_version so tokens/caches refresh.
    sqlx::query(
        r#"
        UPDATE membership SET policy_version = policy_version + 1
        WHERE org_id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    Ok((read_id, write_id))
}

/// Validate slug shape for entity definitions.
pub fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.len() < 2 || slug.len() > 64 {
        return Err("slug must be 2–64 chars".into());
    }
    let mut chars = slug.chars();
    let Some(first) = chars.next() else {
        return Err("empty slug".into());
    };
    if !first.is_ascii_lowercase() {
        return Err("slug must start with a lowercase letter".into());
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err("slug may only contain [a-z0-9_]".into());
    }
    if companyos_authz::CUSTOM_RESERVED_RESOURCES.contains(&slug)
        || matches!(
            slug,
            "entities" | "records" | "packages" | "views" | "layouts" | "scripts"
        )
    {
        return Err("slug is reserved".into());
    }
    Ok(())
}

/// Probe whether a pool can see dynamic permission rows (used by tests).
pub async fn permission_exists(pool: &PgPool, id: &str) -> Result<bool, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM permission_definition WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}
