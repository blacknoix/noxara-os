//! companyos-outbox-relay binary: migrate → relay loop (+ optional HTTP / CLI replay).

use std::net::SocketAddr;
use std::time::Duration;

use clap::{Parser, Subcommand};
use companyos_outbox::relay;
use companyos_outbox_relay::metrics_api::HttpState;
use companyos_outbox_relay::{build_router, new_metrics, publisher};
use companyos_telemetry::init_tracing;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(
    name = "companyos-outbox-relay",
    about = "Outbox → NATS JetStream relay"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Replay dead-letter queue rows back into the outbox.
    Replay {
        /// Replay every unreplayed DLQ row.
        #[arg(long)]
        all: bool,
        /// Replay a single DLQ row by id.
        #[arg(long)]
        id: Option<Uuid>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-outbox-relay");
    let cli = Cli::parse();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://companyos:companyos@127.0.0.1:5432/companyos".into());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    companyos_outbox::migrate(&pool).await?;

    if let Some(Commands::Replay { all, id }) = cli.command {
        return run_replay(&pool, all, id).await;
    }

    let publisher = publisher::publisher_from_env().await?;
    let metrics = new_metrics();

    let http_state = HttpState {
        pool: pool.clone(),
        metrics: metrics.clone(),
    };
    let app = build_router(http_state);
    let addr: SocketAddr = std::env::var("OUTBOX_RELAY_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8090".into())
        .parse()?;

    let poll_ms: u64 = std::env::var("OUTBOX_RELAY_POLL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let batch_size: i64 = std::env::var("OUTBOX_RELAY_BATCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);
    let lag_threshold: u64 = std::env::var("OUTBOX_LAG_ALERT_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);

    info!(%addr, poll_ms, batch_size, "companyos-outbox-relay starting");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let http = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = %e, "outbox relay HTTP server failed");
        }
    });

    let relay_pool = pool.clone();
    let relay_metrics = metrics.clone();
    let relay = tokio::spawn(async move {
        relay::run_relay_loop(
            relay_pool,
            publisher,
            relay_metrics,
            Duration::from_millis(poll_ms),
            batch_size,
            lag_threshold,
        )
        .await;
    });

    tokio::select! {
        _ = http => {},
        _ = relay => {},
    }
    Ok(())
}

async fn run_replay(pool: &sqlx::PgPool, all: bool, id: Option<Uuid>) -> anyhow::Result<()> {
    if let Some(dlq_id) = id {
        let new_id = relay::replay_dlq_row(pool, dlq_id).await?;
        info!(%dlq_id, outbox_id = %new_id, "replayed DLQ row");
        return Ok(());
    }
    if all {
        let rows = relay::list_unreplayed_dlq(pool, 10_000).await?;
        info!(count = rows.len(), "replaying unreplayed DLQ rows");
        for row in rows {
            match relay::replay_dlq_row(pool, row.id).await {
                Ok(new_id) => info!(dlq_id = %row.id, outbox_id = %new_id, "replayed"),
                Err(e) => error!(dlq_id = %row.id, error = %e, "replay failed"),
            }
        }
        return Ok(());
    }
    anyhow::bail!("replay requires --all or --id UUID");
}
