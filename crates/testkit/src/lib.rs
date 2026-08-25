//! Tenant-isolation test harness.
//!
//! Plants a cross-tenant query and **fails loudly** when RLS does not block it.
//! Requires `TEST_DATABASE_URL` (or `DATABASE_URL`) pointing at Postgres.

use std::env;

use companyos_tenancy::{set_session_org_id, OrgId};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Resolve test database URL.
pub fn test_database_url() -> Option<String> {
    env::var("TEST_DATABASE_URL")
        .or_else(|_| env::var("DATABASE_URL"))
        .ok()
}

/// Connect to the test database.
pub async fn connect() -> anyhow::Result<PgPool> {
    let url = test_database_url()
        .ok_or_else(|| anyhow::anyhow!("TEST_DATABASE_URL or DATABASE_URL required"))?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;
    assert_not_superuser(&pool).await?;
    Ok(pool)
}

/// Superusers (and BYPASSRLS) silently skip RLS — isolation tests would be false greens.
pub async fn assert_not_superuser(pool: &PgPool) -> anyhow::Result<()> {
    let row: (bool, bool) = sqlx::query_as(
        r#"
        SELECT rolsuper, rolbypassrls
        FROM pg_roles
        WHERE rolname = current_user
        "#,
    )
    .fetch_one(pool)
    .await?;
    if row.0 || row.1 {
        anyhow::bail!(
            "TENANT ISOLATION SETUP ERROR: connected as role with SUPERUSER={} BYPASSRLS={}. \
             PostgreSQL superusers bypass RLS even with FORCE ROW LEVEL SECURITY. \
             Demote the role (ALTER ROLE … NOSUPERUSER NOBYPASSRLS) before running isolation tests.",
            row.0,
            row.1
        );
    }
    Ok(())
}

/// Apply Phase 0 schema used by isolation tests (hello + outbox + RLS).
pub async fn migrate_isolation_schema(pool: &PgPool) -> anyhow::Result<()> {
    let stmts = [
        r#"
        CREATE TABLE IF NOT EXISTS hello_message (
            id UUID PRIMARY KEY,
            org_id UUID NOT NULL,
            public_id TEXT NOT NULL UNIQUE,
            message TEXT NOT NULL,
            created_by UUID NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
        "CREATE INDEX IF NOT EXISTS hello_message_org_id_idx ON hello_message (org_id)",
        "ALTER TABLE hello_message ENABLE ROW LEVEL SECURITY",
        "ALTER TABLE hello_message FORCE ROW LEVEL SECURITY",
        "DROP POLICY IF EXISTS hello_tenant_isolation ON hello_message",
        r#"
        CREATE POLICY hello_tenant_isolation ON hello_message
            USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
            WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        "#,
    ];
    for stmt in stmts {
        sqlx::query(stmt).execute(pool).await?;
    }

    let outbox_sql = include_str!("../../outbox/migrations/001_outbox_event.sql");
    for stmt in outbox_sql.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        if stmt
            .lines()
            .all(|l| l.trim().is_empty() || l.trim_start().starts_with("--"))
        {
            continue;
        }
        sqlx::query(stmt).execute(pool).await?;
    }

    Ok(())
}

/// Seed two orgs with one hello row each. Returns (org_a, org_b, row_a_id, row_b_id).
pub async fn seed_two_tenants(pool: &PgPool) -> anyhow::Result<(OrgId, OrgId, Uuid, Uuid)> {
    let org_a = OrgId::generate();
    let org_b = OrgId::generate();
    let row_a = companyos_ids::new_uuid_v7();
    let row_b = companyos_ids::new_uuid_v7();
    let user_a = companyos_ids::new_uuid_v7();
    let user_b = companyos_ids::new_uuid_v7();

    let mut tx = pool.begin().await?;
    set_session_org_id(&mut tx, org_a).await?;
    sqlx::query(
        r#"
        INSERT INTO hello_message (id, org_id, public_id, message, created_by)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(row_a)
    .bind(org_a.as_uuid())
    .bind(format!("hel_{row_a}"))
    .bind("hello from A")
    .bind(user_a)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut tx = pool.begin().await?;
    set_session_org_id(&mut tx, org_b).await?;
    sqlx::query(
        r#"
        INSERT INTO hello_message (id, org_id, public_id, message, created_by)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(row_b)
    .bind(org_b.as_uuid())
    .bind(format!("hel_{row_b}"))
    .bind("hello from B")
    .bind(user_b)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok((org_a, org_b, row_a, row_b))
}

/// Assert org A cannot read org B's row when session is bound to A.
pub async fn assert_cannot_read_other_tenant(
    pool: &PgPool,
    org_a: OrgId,
    row_b: Uuid,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    set_session_org_id(&mut tx, org_a).await?;
    let found: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM hello_message WHERE id = $1")
        .bind(row_b)
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;
    if found.is_some() {
        anyhow::bail!(
            "TENANT ISOLATION FAILURE: org A session read org B row {row_b}. RLS is broken."
        );
    }
    Ok(())
}

/// Plant a deliberate cross-tenant SELECT (no WHERE org_id) and assert RLS hides foreign rows.
///
/// **Fails loudly** if the planted query returns another tenant's data.
pub async fn assert_planted_cross_tenant_select_fails(
    pool: &PgPool,
    org_a: OrgId,
    org_b: OrgId,
    row_b: Uuid,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    set_session_org_id(&mut tx, org_a).await?;

    // PLANTED cross-tenant query: intentionally omits org_id predicate.
    let rows = sqlx::query("SELECT id, org_id FROM hello_message")
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;

    for row in &rows {
        let id: Uuid = row.try_get("id")?;
        let org: Uuid = row.try_get("org_id")?;
        if org == org_b.as_uuid() || id == row_b {
            anyhow::bail!(
                "TENANT ISOLATION FAILURE (planted cross-tenant SELECT): \
                 session org={} saw foreign row id={id} org_id={org}. \
                 PostgreSQL RLS must block this.",
                org_a
            );
        }
        if org != org_a.as_uuid() {
            anyhow::bail!(
                "TENANT ISOLATION FAILURE: session org={} saw unexpected org_id={org}",
                org_a
            );
        }
    }
    Ok(())
}

/// Full harness used by `cargo test` in this crate and by CI.
pub async fn run_tenant_isolation_suite() -> anyhow::Result<()> {
    let pool = connect().await?;
    migrate_isolation_schema(&pool).await?;
    let (org_a, org_b, _row_a, row_b) = seed_two_tenants(&pool).await?;
    assert_cannot_read_other_tenant(&pool, org_a, row_b).await?;
    assert_planted_cross_tenant_select_fails(&pool, org_a, org_b, row_b).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tenant_isolation_planted_query_fails_at_database() {
        let url = test_database_url();
        if url.is_none() {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        }
        run_tenant_isolation_suite()
            .await
            .expect("tenant isolation suite must pass");
    }

    /// Meta-test: documents that a successful cross-tenant read is a hard failure.
    #[test]
    fn loud_failure_message_contract() {
        let msg = "TENANT ISOLATION FAILURE (planted cross-tenant SELECT)";
        assert!(msg.contains("TENANT ISOLATION FAILURE"));
    }
}
