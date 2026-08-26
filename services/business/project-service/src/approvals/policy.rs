//! Policy CRUD helpers (versioned; never rewrite historical definitions).

use chrono::{DateTime, Utc};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use uuid::Uuid;

use super::types::{ApprovalMode, ApprovalPolicyDto, PolicyDefinition};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PolicyRow {
    pub id: Uuid,
    pub public_id: String,
    pub name: String,
    pub subject_type: String,
    pub is_active: bool,
    pub current_version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn fetch_policy_dto(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    policy_id: Uuid,
) -> Result<Option<ApprovalPolicyDto>, sqlx::Error> {
    let row: Option<PolicyRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, name, subject_type, is_active, current_version, created_at, updated_at
        FROM operations_approval_policy
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(policy_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let raw: Option<(serde_json::Value,)> = sqlx::query_as(
        r#"
        SELECT definition_json
        FROM operations_approval_policy_version
        WHERE org_id = $1 AND policy_id = $2 AND version = $3
        "#,
    )
    .bind(org_id)
    .bind(row.id)
    .bind(row.current_version)
    .fetch_optional(&mut **tx)
    .await?;
    let def = raw
        .and_then(|(v,)| serde_json::from_value(v).ok())
        .unwrap_or(PolicyDefinition {
            mode: ApprovalMode::Any,
            match_criteria: Default::default(),
            steps: vec![],
        });
    Ok(Some(ApprovalPolicyDto {
        id: row.public_id,
        name: row.name,
        subject_type: row.subject_type,
        is_active: row.is_active,
        current_version: row.current_version,
        definition: def,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    }))
}

pub async fn find_matching_policy(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    subject_type: &str,
    ctx: &super::routing::RouteContext,
) -> Result<Option<(PolicyRow, Uuid, PolicyDefinition)>, sqlx::Error> {
    let rows: Vec<PolicyRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, name, subject_type, is_active, current_version, created_at, updated_at
        FROM operations_approval_policy
        WHERE org_id = $1 AND subject_type = $2 AND is_active = true
        ORDER BY created_at ASC
        "#,
    )
    .bind(org_id)
    .bind(subject_type)
    .fetch_all(&mut **tx)
    .await?;

    for row in rows {
        let version_row: Option<(Uuid, serde_json::Value)> = sqlx::query_as(
            r#"
            SELECT id, definition_json
            FROM operations_approval_policy_version
            WHERE org_id = $1 AND policy_id = $2 AND version = $3
            "#,
        )
        .bind(org_id)
        .bind(row.id)
        .bind(row.current_version)
        .fetch_optional(&mut **tx)
        .await?;
        let Some((version_id, def_json)) = version_row else {
            continue;
        };
        let Ok(def) = serde_json::from_value::<PolicyDefinition>(def_json) else {
            continue;
        };
        if super::routing::matches_criteria(&def.match_criteria, ctx) {
            return Ok(Some((row, version_id, def)));
        }
    }
    Ok(None)
}

pub async fn insert_policy(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    name: &str,
    subject_type: &str,
    def: &PolicyDefinition,
    published_by: Uuid,
) -> Result<(Uuid, String), sqlx::Error> {
    let public_id = PublicId::generate(IdKind::ApprovalPolicy);
    let policy_id = public_id.uuid();
    let version_id = new_uuid_v7();
    let def_json = serde_json::to_value(def).unwrap_or_default();

    sqlx::query(
        r#"
        INSERT INTO operations_approval_policy (
            id, org_id, public_id, name, subject_type, is_active, current_version
        ) VALUES ($1,$2,$3,$4,$5,true,1)
        "#,
    )
    .bind(policy_id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(name)
    .bind(subject_type)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO operations_approval_policy_version (
            id, org_id, policy_id, version, definition_json, published_by
        ) VALUES ($1,$2,$3,1,$4,$5)
        "#,
    )
    .bind(version_id)
    .bind(org_id)
    .bind(policy_id)
    .bind(def_json)
    .bind(published_by)
    .execute(&mut **tx)
    .await?;

    Ok((policy_id, public_id.as_str()))
}

/// Publish a new immutable version; never mutates prior definition_json rows.
pub async fn publish_new_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    policy_id: Uuid,
    def: &PolicyDefinition,
    published_by: Uuid,
) -> Result<i32, sqlx::Error> {
    let current: i32 = sqlx::query_scalar(
        "SELECT current_version FROM operations_approval_policy WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(policy_id)
    .fetch_one(&mut **tx)
    .await?;
    let next = current + 1;
    let version_id = new_uuid_v7();
    let def_json = serde_json::to_value(def).unwrap_or_default();

    sqlx::query(
        r#"
        INSERT INTO operations_approval_policy_version (
            id, org_id, policy_id, version, definition_json, published_by
        ) VALUES ($1,$2,$3,$4,$5,$6)
        "#,
    )
    .bind(version_id)
    .bind(org_id)
    .bind(policy_id)
    .bind(next)
    .bind(def_json)
    .bind(published_by)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE operations_approval_policy
        SET current_version = $3, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(policy_id)
    .bind(next)
    .execute(&mut **tx)
    .await?;

    Ok(next)
}
