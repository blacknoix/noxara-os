//! CompanyOS **Finance** service — Phase 1.5.
//!
//! Standalone network service (own binary, own port) mounted under
//! `/api/v1/finance/...`. Shares the core Postgres database (reads
//! `organization` / `membership` / `role_permission` / `org_role` for authz)
//! but owns its own `finance_*` schema and never touches CRM/`sales_*` tables.

use std::net::SocketAddr;

use companyos_finance::{auth, build_router, migrate, state::AppState};
use companyos_telemetry::init_tracing;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-finance");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    migrate(&pool).await?;
    // Ensure outbox + DLQ schema exists (shared DB). Production publishing is
    // companyos-outbox-relay; optional MemoryPublisher when OUTBOX_EMBEDDED_RELAY=1.
    companyos_outbox::migrate(&pool).await?;
    companyos_outbox::spawn::spawn_embedded_relay_if_configured(pool.clone());

    let ring = auth::build_keyring();
    auth::load_rotated_keys(&pool, &ring).await?;
    let state = AppState::new(pool, ring);
    let app = build_router(state);

    let addr: SocketAddr = std::env::var("FINANCE_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8083".into())
        .parse()?;
    let local = auth::local_auth_enabled();
    info!(%addr, local_auth = local, "companyos-finance listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
