//! First-use pipeline seeding.
//!
//! `ensure_default_pipeline` is idempotent: if the org already has a
//! pipeline, it is returned unchanged. Otherwise it copies stage names from
//! `workspace_seed_pipeline_stage` (set by core's OrgProvisioning) when
//! present, or falls back to a sane default. The last stage(s) named
//! "Won"/"Lost" (case-insensitive) are marked `is_won` / `is_lost`.

use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use sqlx::PgConnection;
use uuid::Uuid;

const DEFAULT_STAGES: &[&str] = &["New", "Qualified", "Proposal", "Negotiation", "Won", "Lost"];

/// Ensure the org has a default pipeline with stages, returning its id.
/// Caller must have already bound `app.org_id` (RLS) on `conn`.
pub async fn ensure_default_pipeline(
    conn: &mut PgConnection,
    org_id: Uuid,
) -> Result<Uuid, sqlx::Error> {
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>(
        r#"
        SELECT id FROM sales_pipeline
        WHERE org_id = $1 AND deleted_at IS NULL
        ORDER BY is_default DESC, created_at ASC
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .fetch_optional(&mut *conn)
    .await?
    {
        return Ok(id);
    }

    let pipeline_id = new_uuid_v7();
    let pipeline_public = PublicId::new(IdKind::Pipeline, pipeline_id);
    sqlx::query(
        r#"
        INSERT INTO sales_pipeline (id, org_id, public_id, name, is_default)
        VALUES ($1, $2, $3, 'Default Pipeline', true)
        "#,
    )
    .bind(pipeline_id)
    .bind(org_id)
    .bind(pipeline_public.as_str())
    .execute(&mut *conn)
    .await?;

    let seed_rows: Vec<(String, i32)> = sqlx::query_as(
        r#"
        SELECT name, position FROM workspace_seed_pipeline_stage
        WHERE org_id = $1
        ORDER BY position ASC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *conn)
    .await?;

    let stages: Vec<(String, i32)> = if seed_rows.is_empty() {
        DEFAULT_STAGES
            .iter()
            .enumerate()
            .map(|(i, n)| (n.to_string(), i as i32))
            .collect()
    } else {
        seed_rows
    };

    for (name, position) in &stages {
        let is_won = name.eq_ignore_ascii_case("won");
        let is_lost = name.eq_ignore_ascii_case("lost");
        let probability: i32 = if is_won { 100 } else { 0 };
        let stage_id = new_uuid_v7();
        let stage_public = PublicId::new(IdKind::Stage, stage_id);
        sqlx::query(
            r#"
            INSERT INTO sales_pipeline_stage (
                id, org_id, public_id, pipeline_id, name, position, probability, is_won, is_lost
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(stage_id)
        .bind(org_id)
        .bind(stage_public.as_str())
        .bind(pipeline_id)
        .bind(name)
        .bind(position)
        .bind(probability)
        .bind(is_won)
        .bind(is_lost)
        .execute(&mut *conn)
        .await?;
    }

    Ok(pipeline_id)
}

/// First non-won/non-lost stage (lowest position) for new deals, falling
/// back to the lowest-position stage overall if every stage is terminal.
pub async fn default_open_stage(
    conn: &mut PgConnection,
    org_id: Uuid,
    pipeline_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>(
        r#"
        SELECT id FROM sales_pipeline_stage
        WHERE org_id = $1 AND pipeline_id = $2 AND deleted_at IS NULL
          AND NOT is_won AND NOT is_lost
        ORDER BY position ASC
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(pipeline_id)
    .fetch_optional(&mut *conn)
    .await?
    {
        return Ok(Some(id));
    }
    let fallback: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT id FROM sales_pipeline_stage
        WHERE org_id = $1 AND pipeline_id = $2 AND deleted_at IS NULL
        ORDER BY position ASC
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(pipeline_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(fallback.map(|(id,)| id))
}
