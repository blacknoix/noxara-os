use std::net::SocketAddr;

use companyos_analytics::{auth, build_router, migrate, state::AppState};
use companyos_telemetry::init_tracing;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-analytics");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    migrate(&pool).await?;

    let ring = auth::build_keyring();
    let _ = auth::load_rotated_keys(&pool, &ring).await;
    let state = AppState::new(pool, ring);
    let app = build_router(state);

    let addr: SocketAddr = std::env::var("ANALYTICS_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8087".into())
        .parse()?;
    info!(%addr, "companyos-analytics listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
