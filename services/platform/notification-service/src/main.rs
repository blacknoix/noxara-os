//! CompanyOS notification service — Phase 1.8.

use std::net::SocketAddr;
use std::time::Duration;

use companyos_notification::{auth, build_router, digest, migrate, state::AppState};
use companyos_telemetry::init_tracing;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-notification");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    migrate(&pool).await?;

    let ring = auth::build_keyring();
    let _ = auth::load_rotated_keys(&pool, &ring).await;
    let state = AppState::new(pool.clone(), ring);
    let app = build_router(state);

    // Background digest flusher (quiet-hours deferred email).
    let digest_pool = pool.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(300));
        loop {
            tick.tick().await;
            match digest::run_deferred_digest(&digest_pool).await {
                Ok(n) if n > 0 => tracing::info!(processed = n, "notification digest flushed"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e.detail, "notification digest failed"),
            }
        }
    });

    let addr: SocketAddr = std::env::var("NOTIFICATION_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8085".into())
        .parse()?;
    let local = auth::local_auth_enabled();
    info!(%addr, local_auth = local, "companyos-notification listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
