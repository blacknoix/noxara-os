//! NATS JetStream publisher + logging memory fallback for outbox relay.

use std::sync::Arc;

use async_trait::async_trait;
use companyos_outbox::relay::{EventPublisher, STREAM_NAME};
use tracing::{info, warn};

/// Publishes to NATS JetStream stream `COMPANYOS_EVENTS`.
pub struct NatsPublisher {
    js: async_nats::jetstream::Context,
}

impl NatsPublisher {
    pub async fn connect(nats_url: &str) -> anyhow::Result<Self> {
        let client = async_nats::connect(nats_url).await?;
        let js = async_nats::jetstream::new(client);
        // Ensure stream exists (idempotent create-or-get).
        let _ = js
            .get_or_create_stream(async_nats::jetstream::stream::Config {
                name: STREAM_NAME.to_string(),
                subjects: vec!["companyos.>".to_string()],
                ..Default::default()
            })
            .await;
        Ok(Self { js })
    }
}

#[async_trait]
impl EventPublisher for NatsPublisher {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), String> {
        self.js
            .publish(subject.to_string(), bytes::Bytes::copy_from_slice(payload))
            .await
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Dev fallback: logs each publish and records deliveries (no NATS).
pub type DeliveryLog = Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

#[derive(Default, Clone)]
pub struct MemoryPublisher {
    pub deliveries: DeliveryLog,
}

impl MemoryPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn delivered_count(&self) -> usize {
        self.deliveries.lock().unwrap().len()
    }
}

#[async_trait]
impl EventPublisher for MemoryPublisher {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), String> {
        info!(
            %subject,
            bytes = payload.len(),
            "outbox MemoryPublisher (NATS_URL unset) — event logged, not relayed to NATS"
        );
        self.deliveries
            .lock()
            .unwrap()
            .push((subject.to_string(), payload.to_vec()));
        Ok(())
    }
}

/// Build publisher from env: `NATS_URL` → JetStream, else logging memory.
pub async fn publisher_from_env() -> anyhow::Result<Arc<dyn EventPublisher>> {
    if let Ok(url) = std::env::var("NATS_URL") {
        if !url.is_empty() {
            info!(%url, "outbox relay using NATS JetStream publisher");
            let nats = NatsPublisher::connect(&url).await?;
            return Ok(Arc::new(nats));
        }
    }
    warn!("NATS_URL unset — using MemoryPublisher (dev fallback)");
    Ok(Arc::new(MemoryPublisher::new()))
}
