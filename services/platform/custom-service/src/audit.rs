//! Audit trail writes — always inside the same transaction as the domain write.

use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn insert_audit<'e, E>(
    executor: E,
    org_id: Uuid,
    actor_user_id: Uuid,
    actor_on_behalf_of: Uuid,
    actor_is_ai: bool,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    metadata: serde_json::Value,
) -> Result<(), sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query(
        r#"
        INSERT INTO audit_entry (
            id, org_id, actor_user_id, actor_on_behalf_of, actor_is_ai,
            action, resource_type, resource_id, metadata
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(companyos_ids::new_uuid_v7())
    .bind(org_id)
    .bind(actor_user_id)
    .bind(actor_on_behalf_of)
    .bind(actor_is_ai)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(metadata)
    .execute(executor)
    .await?;
    Ok(())
}
