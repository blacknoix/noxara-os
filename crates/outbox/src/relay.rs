//! Outbox → NATS JetStream relay (Phase 1.8).
//!
//! - Claims unpublished rows with `FOR UPDATE SKIP LOCKED` under `app.outbox_relay=1`
//! - Publishes at-least-once via [`EventPublisher`]
//! - Marks `published_at` on success; moves to `outbox_dlq` after max attempts
//! - Emits lag metric (log + [`RelayMetrics`]) for the runbook alert

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{OutboxError, OutboxEvent};

/// JetStream stream / subject conventions (see `scripts/nats-bootstrap.sh`).
pub const STREAM_NAME: &str = "COMPANYOS_EVENTS";
pub const SUBJECT_FILTER: &str = "companyos.>";
pub const DLQ_STREAM_NAME: &str = "COMPANYOS_EVENTS_DLQ";
pub const DLQ_SUBJECT: &str = "companyos.dlq.>";
pub const CONSUMER_DURABLE: &str = "platform-consumers";

/// Pluggable publisher (NATS JetStream in prod; in-memory in tests).
#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), String>;
}

/// In-memory publisher for unit/integration tests (records deliveries).
pub type DeliveryLog = Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

#[derive(Default, Clone)]
pub struct MemoryPublisher {
    pub deliveries: DeliveryLog,
    pub fail_times: Arc<std::sync::Mutex<u32>>,
}

impl MemoryPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn delivered_count(&self) -> usize {
        self.deliveries.lock().unwrap().len()
    }

    pub fn set_fail_times(&self, n: u32) {
        *self.fail_times.lock().unwrap() = n;
    }
}

#[async_trait]
impl EventPublisher for MemoryPublisher {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), String> {
        let mut fails = self.fail_times.lock().unwrap();
        if *fails > 0 {
            *fails -= 1;
            return Err("forced publish failure".into());
        }
        drop(fails);
        self.deliveries
            .lock()
            .unwrap()
            .push((subject.to_string(), payload.to_vec()));
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct RelayMetrics {
    pub published: AtomicU64,
    pub dlq: AtomicU64,
    pub lag: AtomicU64,
    pub batches: AtomicU64,
}

impl RelayMetrics {
    pub fn snapshot(&self) -> RelaySnapshot {
        RelaySnapshot {
            published: self.published.load(Ordering::Relaxed),
            dlq: self.dlq.load(Ordering::Relaxed),
            lag: self.lag.load(Ordering::Relaxed),
            batches: self.batches.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelaySnapshot {
    pub published: u64,
    pub dlq: u64,
    pub lag: u64,
    pub batches: u64,
}

/// Enable cross-tenant relay mode for the current transaction.
pub async fn set_relay_session(tx: &mut Transaction<'_, Postgres>) -> Result<(), OutboxError> {
    sqlx::query("SELECT set_config('app.outbox_relay', '1', true)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Claim a batch of unpublished events (SKIP LOCKED).
pub async fn claim_unpublished(
    tx: &mut Transaction<'_, Postgres>,
    limit: i64,
) -> Result<Vec<OutboxEvent>, OutboxError> {
    set_relay_session(tx).await?;
    let rows: Vec<OutboxEvent> = sqlx::query_as(
        r#"
        SELECT id, org_id, subject, payload, idempotency_key, created_at, published_at
        FROM outbox_event
        WHERE published_at IS NULL
        ORDER BY created_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// Count unpublished events (lag).
pub async fn unpublished_lag(pool: &PgPool) -> Result<u64, OutboxError> {
    let mut tx = pool.begin().await?;
    set_relay_session(&mut tx).await?;
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM outbox_event WHERE published_at IS NULL")
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(count as u64)
}

/// Oldest unpublished created_at (for lag age alerting).
pub async fn oldest_unpublished_at(pool: &PgPool) -> Result<Option<DateTime<Utc>>, OutboxError> {
    let mut tx = pool.begin().await?;
    set_relay_session(&mut tx).await?;
    let row: Option<(DateTime<Utc>,)> = sqlx::query_as(
        "SELECT created_at FROM outbox_event WHERE published_at IS NULL ORDER BY created_at ASC LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row.map(|r| r.0))
}

async fn mark_published_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<(), OutboxError> {
    sqlx::query("UPDATE outbox_event SET published_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Move a failed event to the DLQ (keeps outbox row unpublished until replay).
pub async fn move_to_dlq(
    tx: &mut Transaction<'_, Postgres>,
    event: &OutboxEvent,
    error: &str,
) -> Result<Uuid, OutboxError> {
    set_relay_session(tx).await?;
    let id = companyos_ids::new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO outbox_dlq (id, outbox_id, org_id, subject, payload, idempotency_key, error, attempts)
        VALUES ($1, $2, $3, $4, $5, $6, $7, 1)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(id)
    .bind(event.id)
    .bind(event.org_id)
    .bind(&event.subject)
    .bind(&event.payload)
    .bind(&event.idempotency_key)
    .bind(error)
    .execute(&mut **tx)
    .await?;
    // Mark original published so it doesn't loop forever; replay re-inserts.
    mark_published_tx(tx, event.id).await?;
    Ok(id)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DlqRow {
    pub id: Uuid,
    pub outbox_id: Uuid,
    pub org_id: Uuid,
    pub subject: String,
    pub payload: serde_json::Value,
    pub idempotency_key: String,
    pub error: String,
    pub attempts: i32,
    pub created_at: DateTime<Utc>,
    pub replayed_at: Option<DateTime<Utc>>,
}

pub async fn list_unreplayed_dlq(pool: &PgPool, limit: i64) -> Result<Vec<DlqRow>, OutboxError> {
    let mut tx = pool.begin().await?;
    set_relay_session(&mut tx).await?;
    let rows: Vec<DlqRow> = sqlx::query_as(
        r#"
        SELECT id, outbox_id, org_id, subject, payload, idempotency_key, error, attempts, created_at, replayed_at
        FROM outbox_dlq
        WHERE replayed_at IS NULL
        ORDER BY created_at ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(rows)
}

/// Replay one DLQ row: re-insert into outbox as unpublished and mark DLQ replayed.
pub async fn replay_dlq_row(pool: &PgPool, dlq_id: Uuid) -> Result<Uuid, OutboxError> {
    let mut tx = pool.begin().await?;
    set_relay_session(&mut tx).await?;
    let row: Option<DlqRow> = sqlx::query_as(
        r#"
        SELECT id, outbox_id, org_id, subject, payload, idempotency_key, error, attempts, created_at, replayed_at
        FROM outbox_dlq WHERE id = $1 FOR UPDATE
        "#,
    )
    .bind(dlq_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(OutboxError::Relay(format!("dlq row {dlq_id} not found")));
    };
    if row.replayed_at.is_some() {
        return Err(OutboxError::Relay(format!(
            "dlq row {dlq_id} already replayed"
        )));
    }
    let new_id = companyos_ids::new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO outbox_event (id, org_id, subject, payload, idempotency_key, created_at)
        VALUES ($1, $2, $3, $4, $5, now())
        "#,
    )
    .bind(new_id)
    .bind(row.org_id)
    .bind(&row.subject)
    .bind(&row.payload)
    .bind(&row.idempotency_key)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE outbox_dlq SET replayed_at = now() WHERE id = $1")
        .bind(dlq_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(new_id)
}

/// Publish one claimed batch. Caller owns the transaction until after publish.
pub async fn publish_batch(
    pool: &PgPool,
    publisher: &dyn EventPublisher,
    metrics: &RelayMetrics,
    limit: i64,
    max_attempts_before_dlq: u32,
) -> Result<usize, OutboxError> {
    let mut tx = pool.begin().await?;
    let batch = claim_unpublished(&mut tx, limit).await?;
    if batch.is_empty() {
        tx.commit().await?;
        return Ok(0);
    }
    // Hold locks while publishing (at-least-once: crash before commit → retry).
    let mut published = 0usize;
    for event in &batch {
        let bytes =
            serde_json::to_vec(&event.payload).map_err(|e| OutboxError::Relay(e.to_string()))?;
        let mut last_err = None;
        for attempt in 1..=max_attempts_before_dlq {
            match publisher.publish(&event.subject, &bytes).await {
                Ok(()) => {
                    mark_published_tx(&mut tx, event.id).await?;
                    metrics.published.fetch_add(1, Ordering::Relaxed);
                    published += 1;
                    last_err = None;
                    break;
                }
                Err(e) => {
                    warn!(
                        outbox_id = %event.id,
                        attempt,
                        error = %e,
                        "outbox publish failed"
                    );
                    last_err = Some(e);
                }
            }
        }
        if let Some(e) = last_err {
            move_to_dlq(&mut tx, event, &e).await?;
            metrics.dlq.fetch_add(1, Ordering::Relaxed);
            error!(
                outbox_id = %event.id,
                subject = %event.subject,
                error = %e,
                "outbox event moved to DLQ"
            );
        }
    }
    tx.commit().await?;
    metrics.batches.fetch_add(1, Ordering::Relaxed);
    Ok(published)
}

/// One relay tick: publish + refresh lag metric (alert via log when lag > threshold).
pub async fn relay_once(
    pool: &PgPool,
    publisher: &dyn EventPublisher,
    metrics: &RelayMetrics,
    batch_size: i64,
    lag_alert_threshold: u64,
) -> Result<usize, OutboxError> {
    let n = publish_batch(pool, publisher, metrics, batch_size, 3).await?;
    let lag = unpublished_lag(pool).await?;
    metrics.lag.store(lag, Ordering::Relaxed);
    if lag > lag_alert_threshold {
        let oldest = oldest_unpublished_at(pool).await?;
        warn!(
            lag,
            ?oldest,
            threshold = lag_alert_threshold,
            "OUTBOX_LAG_ALERT: unpublished outbox events exceed threshold — see docs/runbooks/outbox-lag.md"
        );
    } else if n > 0 {
        info!(published = n, lag, "outbox relay batch ok");
    }
    Ok(n)
}

/// Background loop used by service binaries and `companyos-outbox-relay`.
pub async fn run_relay_loop(
    pool: PgPool,
    publisher: Arc<dyn EventPublisher>,
    metrics: Arc<RelayMetrics>,
    poll_interval: Duration,
    batch_size: i64,
    lag_alert_threshold: u64,
) {
    loop {
        if let Err(e) = relay_once(
            &pool,
            publisher.as_ref(),
            metrics.as_ref(),
            batch_size,
            lag_alert_threshold,
        )
        .await
        {
            error!(error = %e, "outbox relay tick failed");
        }
        tokio::time::sleep(poll_interval).await;
    }
}
