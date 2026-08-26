//! Temporal worker host for the Phase 1.8 workflow catalogue.
//!
//! Heartbeats Temporal like `companyos-project-worker`. Uses
//! `TEMPORAL_NAMESPACE` (default `companyos-local`; CI uses `companyos-ci`) —
//! namespaces must never be shared across environments.

use std::net::SocketAddr;
use std::time::Duration;

use companyos_telemetry::init_tracing;
use companyos_workflow_host::{build_router, catalogue, temporal_namespace, TASK_QUEUE};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-workflow-host");

    let addr_temporal =
        std::env::var("TEMPORAL_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7233".into());
    let namespace = temporal_namespace();
    let http_addr: SocketAddr = std::env::var("WORKFLOW_HOST_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8091".into())
        .parse()?;

    info!(
        %addr_temporal,
        %namespace,
        %TASK_QUEUE,
        workflows = ?catalogue::WorkflowType::all()
            .iter()
            .map(|w| w.as_str())
            .collect::<Vec<_>>(),
        "companyos-workflow-host starting"
    );

    let app = build_router();
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    let http = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let heartbeat = tokio::spawn(async move {
        loop {
            match tokio::net::TcpStream::connect(parse_addr(&addr_temporal)).await {
                Ok(_) => {
                    info!(
                        %addr_temporal,
                        %namespace,
                        %TASK_QUEUE,
                        "Temporal reachable — workflow host heartbeat"
                    );
                    // Activity stubs would poll workflows here; durable
                    // semantics are covered by catalogue unit tests.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                Err(e) => {
                    warn!(%addr_temporal, error = %e, "Temporal unreachable; retrying");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });

    tokio::select! {
        _ = http => {},
        _ = heartbeat => {},
    }
    Ok(())
}

fn parse_addr(addr: &str) -> (String, u16) {
    let host = addr.split(':').next().unwrap_or("127.0.0.1").to_string();
    let port = addr
        .split(':')
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(7233);
    (host, port)
}
