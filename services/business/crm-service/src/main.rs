//! CompanyOS **CRM / Sales** service — Phase 1.4.
//!
//! Standalone network service (own binary, own port) mounted under
//! `/api/v1/sales/...`. Shares the core Postgres database (reads
//! `organization` / `membership` / `role_permission` for authz) but owns
//! its own `sales_*` schema and never touches finance tables.

use std::net::SocketAddr;

use companyos_crm::{auth, build_router, migrate, state::AppState};
use companyos_telemetry::init_tracing;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-crm");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    migrate(&pool).await?;

    let ring = auth::build_keyring();
    auth::load_rotated_keys(&pool, &ring).await?;
    let state = AppState::new(pool, ring);
    let app = build_router(state);

    let addr: SocketAddr = std::env::var("CRM_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8082".into())
        .parse()?;
    let local = auth::local_auth_enabled();
    info!(%addr, local_auth = local, "companyos-crm listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
