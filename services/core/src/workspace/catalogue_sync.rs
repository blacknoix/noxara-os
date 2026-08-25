//! Sync `permission_definition` rows from the Rust catalogue.
//! CI fails when DB and code diverge (see workspace_phase12 tests).

use companyos_authz::PERMISSION_CATALOGUE;
use sqlx::PgPool;

pub async fn sync(pool: &PgPool) -> anyhow::Result<()> {
    for p in PERMISSION_CATALOGUE {
        sqlx::query(
            r#"
            INSERT INTO permission_definition (id, context, resource, action, description, sensitive)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO UPDATE SET
                context = EXCLUDED.context,
                resource = EXCLUDED.resource,
                action = EXCLUDED.action,
                description = EXCLUDED.description,
                sensitive = EXCLUDED.sensitive
            "#,
        )
        .bind(p.id)
        .bind(p.context)
        .bind(p.resource)
        .bind(p.action)
        .bind(p.description)
        .bind(p.sensitive)
        .execute(pool)
        .await?;
    }

    // Remove stale rows not in catalogue.
    let ids: Vec<&str> = PERMISSION_CATALOGUE.iter().map(|p| p.id).collect();
    sqlx::query("DELETE FROM permission_definition WHERE NOT (id = ANY($1))")
        .bind(&ids)
        .execute(pool)
        .await?;

    Ok(())
}

/// Assert DB permission_definition matches catalogue exactly. Used by CI tests.
pub async fn assert_matches_catalogue(pool: &PgPool) -> anyhow::Result<()> {
    let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT id, context, resource, action, sensitive FROM permission_definition ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    let mut expected: Vec<_> = PERMISSION_CATALOGUE
        .iter()
        .map(|p| {
            (
                p.id.to_string(),
                p.context.to_string(),
                p.resource.to_string(),
                p.action.to_string(),
                p.sensitive,
            )
        })
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));

    if rows.len() != expected.len() {
        anyhow::bail!(
            "permission_definition count {} != catalogue {}; DB={:?} code={:?}",
            rows.len(),
            expected.len(),
            rows.iter().map(|r| &r.0).collect::<Vec<_>>(),
            expected.iter().map(|r| &r.0).collect::<Vec<_>>()
        );
    }
    for (db, code) in rows.iter().zip(expected.iter()) {
        if db != code {
            anyhow::bail!("permission_definition divergence: db={db:?} code={code:?}");
        }
    }
    Ok(())
}
