//! Seed a sandbox org + API key for Phase 3.3 public API tests.
//!
//! Usage: `COMPANYOS_LOCAL_AUTH=1 cargo run -p companyos-core --example seed_sandbox`
//! Writes `.tmp/sandbox.env`.

use std::path::PathBuf;

use companyos_auth_token::KeyRing;
use companyos_core::governance::api_keys;
use companyos_core::migrate;
use companyos_core::state::AppState;
use companyos_tenancy::{set_session_org_id, OrgId};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&db)
        .await?;
    migrate(&pool).await?;

    let ring = KeyRing::from_secret(
        std::env::var("AUTH_JWT_SECRET").unwrap_or_else(|_| "local-dev-only-change-me".into()),
    );
    let _state = AppState::new(pool.clone(), ring);

    let org_row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, public_id FROM organization ORDER BY created_at ASC LIMIT 1")
            .fetch_optional(&pool)
            .await?;

    let (org_uuid, org_public) = if let Some(r) = org_row {
        r
    } else {
        anyhow::bail!("no organization found — run scripts/seed-dev.sh first");
    };
    let org_id = OrgId::new(org_uuid);

    let owner: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT user_id FROM membership
        WHERE org_id = $1 AND role = 'owner' AND revoked_at IS NULL
        LIMIT 1
        "#,
    )
    .bind(org_uuid)
    .fetch_optional(&pool)
    .await?;
    let Some((owner_id,)) = owner else {
        anyhow::bail!("no owner membership for sandbox org");
    };

    let mut tx = pool.begin().await?;
    set_session_org_id(&mut tx, org_id).await?;
    let scopes = vec![
        "sales.customer.read".into(),
        "sales.customer.create".into(),
        "finance.invoice.read".into(),
        "finance.invoice.create".into(),
        "finance.invoice.issue".into(),
        "admin.webhook.read".into(),
        "admin.webhook.write".into(),
    ];
    let (_view, secret) = api_keys::create(
        &mut tx,
        org_id,
        owner_id,
        "sandbox-public-api",
        &scopes,
        None,
        "seed-sandbox",
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    tx.commit().await?;

    let out = PathBuf::from(".tmp/sandbox.env");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        &out,
        format!(
            "SANDBOX_ORG_ID={org_public}\nSANDBOX_API_KEY={secret}\nCOMPANYOS_API_URL=http://127.0.0.1:8080\n"
        ),
    )?;
    println!("wrote {}", out.display());
    Ok(())
}
