//! Optional embedded outbox relay for services that write events.
//!
//! **Production path:** run the dedicated `companyos-outbox-relay` binary with
//! NATS JetStream (started by `scripts/dev-up`). That binary owns publishing.
//!
//! **Dev convenience:** set `OUTBOX_EMBEDDED_RELAY=1` to spawn a background loop
//! in-process using [`MemoryPublisher`] (log-only / in-memory deliveries). This
//! avoids pulling `async-nats` into every service binary.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tracing::info;

use crate::relay::{self, MemoryPublisher, RelayMetrics};

/// Spawn an embedded MemoryPublisher relay when `OUTBOX_EMBEDDED_RELAY=1`.
///
/// Returns `true` if a background task was spawned. Primary production publisher
/// remains `companyos-outbox-relay` against the shared database.
pub fn spawn_embedded_relay_if_configured(pool: PgPool) -> bool {
    if std::env::var("OUTBOX_EMBEDDED_RELAY").ok().as_deref() != Some("1") {
        return false;
    }
    info!(
        "OUTBOX_EMBEDDED_RELAY=1 — spawning in-process MemoryPublisher loop \
         (production uses companyos-outbox-relay + NATS)"
    );
    let metrics = Arc::new(RelayMetrics::default());
    let publisher: Arc<dyn relay::EventPublisher> = Arc::new(MemoryPublisher::new());
    tokio::spawn(async move {
        relay::run_relay_loop(
            pool,
            publisher,
            metrics,
            Duration::from_secs(2),
            50,
            100,
        )
        .await;
    });
    true
}
