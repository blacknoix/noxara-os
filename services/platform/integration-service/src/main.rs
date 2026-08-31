//! CompanyOS outbound webhook integration service — Phase 3.3.
//!
//! Polls pending `webhook_delivery` rows and optionally consumes NATS JetStream
//! as durable consumer `integration--outbound-webhooks`.

use std::net::SocketAddr;
use std::time::Duration;

use companyos_integration::crypto::WebhookDecryptor;
use companyos_integration::dispatcher::{run_nats_consumer, run_poll_loop};
use companyos_integration::{build_router, migrate, AppState};
use companyos_telemetry::init_tracing;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-integration");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    migrate(&pool).await?;

    let decryptor = WebhookDecryptor::from_env()?;
    let state = AppState::new(pool.clone(), decryptor.clone());
    let app = build_router(state);

    let poll_ms: u64 = std::env::var("INTEGRATION_POLL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000);

    let poll_pool = pool.clone();
    let poll_decryptor = decryptor.clone();
    tokio::spawn(async move {
        run_poll_loop(poll_pool, poll_decryptor, Duration::from_millis(poll_ms)).await;
    });

    if let Ok(nats_url) = std::env::var("NATS_URL") {
        if !nats_url.is_empty() {
            let nats_pool = pool.clone();
            let url = nats_url.clone();
            tokio::spawn(async move {
                info!(%url, consumer = "integration--outbound-webhooks", "starting NATS consumer");
                if let Err(e) = run_nats_consumer(nats_pool, &url).await {
                    tracing::error!(error = %e, "NATS consumer exited");
                }
            });
        }
    }

    let addr: SocketAddr = std::env::var("INTEGRATION_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8095".into())
        .parse()?;
    info!(%addr, "companyos-integration listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
