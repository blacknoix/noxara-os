//! Migrate core schema and run OrgProvisioning for the fixed local seed org.
//!
//! Used by `scripts/seed-dev.sh` after inserting the Acme Demo org + users:
//!
//! ```bash
//! cargo run -q -p companyos-core --example seed_dev
//! ```

use companyos_tenancy::OrgId;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());
    let org_uuid = uuid::Uuid::parse_str(
        &std::env::var("DEV_ORG_UUID")
            .unwrap_or_else(|_| "018f0000-0000-7000-8000-000000000001".into()),
    )?;
    let owner_uuid = uuid::Uuid::parse_str(
        &std::env::var("DEV_USER_OWNER_UUID")
            .unwrap_or_else(|_| "018f0000-0000-7000-8000-000000000011".into()),
    )?;

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    companyos_core::migrate(&pool).await?;

    let org = OrgId::new(org_uuid);
    let org_public = org.to_public().as_str();
    eprintln!("Provisioning org {org_public} (owner {owner_uuid})…");
    companyos_core::workspace::provisioning::enqueue_and_run(
        &pool, org, owner_uuid, "general", "seed-dev",
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;

    sqlx::query(
        r#"
        UPDATE user_identity
        SET email_verified_at = COALESCE(email_verified_at, now())
        WHERE id = $1
        "#,
    )
    .bind(owner_uuid)
    .execute(&pool)
    .await?;

    eprintln!("OrgProvisioning complete for {org_public}");
    eprintln!("Ledger accounts / CRM pipeline materialize lazily on first write.");
    Ok(())
}
