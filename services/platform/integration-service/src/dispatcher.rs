//! Dispatch pending webhook deliveries (at-least-once with backoff).
//!
//! Flow: claim pending/failed rows → decrypt secret → SSRF check → POST with
//! HMAC signature → update delivery + endpoint failure bookkeeping → auto-pause
//! after 10 consecutive failures. Emits `admin.webhook.{delivered|failed}.v1`
//! via the outbox for audit.

use std::time::Duration;

use chrono::{DateTime, Utc};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_outbox::insert_event;
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use reqwest::redirect::Policy;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::crypto::WebhookDecryptor;
use crate::sign;
use crate::ssrf;

/// Consecutive endpoint failures before auto-pause.
pub const AUTO_PAUSE_FAILURES: i32 = 10;

/// Max attempts before marking a delivery `dead`.
pub const MAX_ATTEMPTS: i32 = 12;

/// Truncate stored response bodies.
const RESPONSE_BODY_MAX: usize = 2048;

#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("tenancy: {0}")]
    Tenancy(String),
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("http client: {0}")]
    Http(String),
    #[error("outbox: {0}")]
    Outbox(String),
}

#[derive(Debug, Clone, Default)]
pub struct DispatchStats {
    pub claimed: usize,
    pub delivered: usize,
    pub failed: usize,
    pub skipped_ssrf: usize,
}

struct ClaimedDelivery {
    id: Uuid,
    org_id: Uuid,
    endpoint_id: Uuid,
    event_id: Uuid,
    #[allow(dead_code)]
    event_subject: String,
    #[allow(dead_code)]
    event_type: String,
    payload: serde_json::Value,
    attempt: i32,
    endpoint_url: String,
    secret_ciphertext: Vec<u8>,
    endpoint_public_id: String,
    failure_count: i32,
}

/// System actor used for delivery audit events (nil UUID = platform).
pub fn system_actor() -> Actor {
    Actor::human(Uuid::nil())
}

async fn set_dispatch_session(tx: &mut Transaction<'_, Postgres>) -> Result<(), DispatchError> {
    sqlx::query("SELECT set_config('app.webhook_dispatch', '1', true)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn backoff_secs(attempt: i32) -> i64 {
    // attempt is 1-based after increment: 1→1s, 2→2s, 3→4s … capped at 1h
    let exp = (attempt.saturating_sub(1)).min(16) as u32;
    (1_i64 << exp).min(3600)
}

fn truncate_response(body: &str) -> String {
    if body.len() <= RESPONSE_BODY_MAX {
        body.to_string()
    } else {
        format!("{}…", &body[..RESPONSE_BODY_MAX])
    }
}

fn build_http_client() -> Result<reqwest::Client, DispatchError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(5))
        .redirect(Policy::none())
        .build()
        .map_err(|e| DispatchError::Http(e.to_string()))
}

type ClaimedRow = (
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    String,
    String,
    serde_json::Value,
    i32,
    String,
    Vec<u8>,
    String,
    i32,
);

/// Whether SSRF private/loopback checks are relaxed (tests / local echo only).
///
/// Prefer passing this explicitly rather than mutating process-wide env vars —
/// integration tests run in parallel and raced on `COMPANYOS_WEBHOOK_SSRF_ALLOW_PRIVATE`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DispatchOptions {
    pub allow_private: bool,
}

impl DispatchOptions {
    /// Production-safe default: SSRF checks enforced.
    pub fn strict() -> Self {
        Self {
            allow_private: false,
        }
    }

    /// Read once from env (service boot). Prefer [`AppState`] over per-call env reads.
    pub fn from_env() -> Self {
        let allow_private = std::env::var("COMPANYOS_WEBHOOK_SSRF_ALLOW_PRIVATE")
            .map(|v| v == "1")
            .unwrap_or(false);
        Self { allow_private }
    }
}

enum ProcessOutcome {
    Delivered,
    FailedSsrf,
    Failed,
}

/// Claim and process up to `limit` pending/retryable deliveries.
pub async fn dispatch_once(
    pool: &PgPool,
    decryptor: &WebhookDecryptor,
    limit: i64,
    opts: DispatchOptions,
) -> Result<DispatchStats, DispatchError> {
    let client = build_http_client()?;
    let mut stats = DispatchStats::default();

    let mut tx = pool.begin().await?;
    set_dispatch_session(&mut tx).await?;

    let rows: Vec<ClaimedRow> = sqlx::query_as(
        r#"
        SELECT d.id, d.org_id, d.endpoint_id, d.event_id, d.event_subject, d.event_type,
               d.payload, d.attempt, e.url, e.secret_ciphertext, e.public_id, e.failure_count
        FROM webhook_delivery d
        INNER JOIN webhook_endpoint e ON e.id = d.endpoint_id AND e.org_id = d.org_id
        WHERE d.status IN ('pending', 'failed')
          AND (d.next_retry_at IS NULL OR d.next_retry_at <= now())
          AND e.status = 'active'
          AND d.attempt < $2
        ORDER BY d.created_at ASC
        FOR UPDATE OF d SKIP LOCKED
        LIMIT $1
        "#,
    )
    .bind(limit)
    .bind(MAX_ATTEMPTS)
    .fetch_all(&mut *tx)
    .await?;

    let mut claimed = Vec::with_capacity(rows.len());
    for (
        id,
        org_id,
        endpoint_id,
        event_id,
        event_subject,
        event_type,
        payload,
        attempt,
        endpoint_url,
        secret_ciphertext,
        endpoint_public_id,
        failure_count,
    ) in rows
    {
        sqlx::query(
            r#"
            UPDATE webhook_delivery
            SET status = 'delivering', attempt = attempt + 1, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;

        claimed.push(ClaimedDelivery {
            id,
            org_id,
            endpoint_id,
            event_id,
            event_subject,
            event_type,
            payload,
            attempt: attempt + 1,
            endpoint_url,
            secret_ciphertext,
            endpoint_public_id,
            failure_count,
        });
    }
    tx.commit().await?;

    stats.claimed = claimed.len();

    for delivery in claimed {
        match process_one(pool, decryptor, &client, &delivery, opts).await {
            Ok(ProcessOutcome::Delivered) => stats.delivered += 1,
            Ok(ProcessOutcome::FailedSsrf) => {
                stats.failed += 1;
                stats.skipped_ssrf += 1;
            }
            Ok(ProcessOutcome::Failed) | Err(_) => stats.failed += 1,
        }
    }

    Ok(stats)
}

/// Deliver one claimed row. SSRF policy comes from `opts` (not process env) so parallel
/// tests cannot race on `COMPANYOS_WEBHOOK_SSRF_ALLOW_PRIVATE`.
async fn process_one(
    pool: &PgPool,
    decryptor: &WebhookDecryptor,
    client: &reqwest::Client,
    d: &ClaimedDelivery,
    opts: DispatchOptions,
) -> Result<ProcessOutcome, DispatchError> {
    // SSRF: resolve/check before decrypt work is wasted; fail closed.
    if !opts.allow_private {
        if let Err(e) = ssrf::assert_url_safe(&d.endpoint_url) {
            mark_failed(
                pool,
                d,
                None,
                None,
                &format!("ssrf: {e}"),
                true, // permanent for SSRF
            )
            .await?;
            return Ok(ProcessOutcome::FailedSsrf);
        }
    }

    let secret = decryptor
        .decrypt(&d.secret_ciphertext)
        .map_err(|e| DispatchError::Crypto(e.to_string()))?;

    let body = serde_json::to_vec(&d.payload)
        .map_err(|e| DispatchError::Http(format!("serialize payload: {e}")))?;

    // Re-check immediately before connect (DNS rebinding guard).
    if !opts.allow_private {
        if let Err(e) = ssrf::assert_url_safe(&d.endpoint_url) {
            mark_failed(pool, d, None, None, &format!("ssrf: {e}"), true).await?;
            return Ok(ProcessOutcome::FailedSsrf);
        }
    }

    let signature =
        sign::sign_now(&secret, &body).map_err(|e| DispatchError::Http(e.to_string()))?;

    let response = client
        .post(&d.endpoint_url)
        .header("content-type", "application/json")
        .header("user-agent", "CompanyOS-Webhooks/1.0")
        .header("X-CompanyOS-Signature", &signature)
        .header("X-CompanyOS-Delivery", d.id.to_string())
        .header("X-CompanyOS-Event-Id", d.event_id.to_string())
        .body(body)
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status().as_u16() as i32;
            let resp_body = resp.text().await.unwrap_or_default();
            let truncated = truncate_response(&resp_body);
            if (200..300).contains(&status) {
                mark_delivered(pool, d, status, &truncated).await?;
                Ok(ProcessOutcome::Delivered)
            } else {
                mark_failed(
                    pool,
                    d,
                    Some(status),
                    Some(&truncated),
                    &format!("http status {status}"),
                    false,
                )
                .await?;
                Ok(ProcessOutcome::Failed)
            }
        }
        Err(e) => {
            // If redirect / connect somehow hit a blocked path, treat as SSRF-ish.
            let permanent = matches_ssrf_err(&e);
            mark_failed(pool, d, None, None, &e.to_string(), permanent).await?;
            Ok(ProcessOutcome::Failed)
        }
    }
}

fn matches_ssrf_err(_e: &reqwest::Error) -> bool {
    false
}

async fn mark_delivered(
    pool: &PgPool,
    d: &ClaimedDelivery,
    status_code: i32,
    response_body: &str,
) -> Result<(), DispatchError> {
    let org = OrgId::new(d.org_id);
    let mut tx = pool.begin().await?;
    set_dispatch_session(&mut tx).await?;
    set_session_org_id(&mut tx, org)
        .await
        .map_err(|e| DispatchError::Tenancy(e.to_string()))?;

    sqlx::query(
        r#"
        UPDATE webhook_delivery
        SET status = 'delivered', status_code = $2, response_body = $3,
            delivered_at = now(), last_error = NULL, next_retry_at = NULL, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(d.id)
    .bind(status_code)
    .bind(response_body)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE webhook_endpoint
        SET failure_count = 0, last_delivery_at = now(), updated_at = now()
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(d.endpoint_id)
    .bind(d.org_id)
    .execute(&mut *tx)
    .await?;

    let envelope = EventEnvelope::new(
        org,
        Context::Admin,
        "webhook",
        "delivered",
        1,
        system_actor(),
        serde_json::json!({
            "webhook_id": d.endpoint_public_id,
            "delivery_id": PublicId::new(IdKind::WebhookDelivery, d.id).as_str(),
            "event_id": d.event_id,
            "status_code": status_code,
            "attempt": d.attempt,
        }),
    );
    insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| DispatchError::Outbox(e.to_string()))?;

    tx.commit().await?;
    Ok(())
}

async fn mark_failed(
    pool: &PgPool,
    d: &ClaimedDelivery,
    status_code: Option<i32>,
    response_body: Option<&str>,
    error: &str,
    permanent: bool,
) -> Result<(), DispatchError> {
    let org = OrgId::new(d.org_id);
    let mut tx = pool.begin().await?;
    set_dispatch_session(&mut tx).await?;
    set_session_org_id(&mut tx, org)
        .await
        .map_err(|e| DispatchError::Tenancy(e.to_string()))?;

    let dead = permanent || d.attempt >= MAX_ATTEMPTS;
    let status = if dead { "dead" } else { "failed" };
    let next_retry: Option<DateTime<Utc>> = if dead {
        None
    } else {
        Some(Utc::now() + chrono::Duration::seconds(backoff_secs(d.attempt)))
    };

    sqlx::query(
        r#"
        UPDATE webhook_delivery
        SET status = $2, status_code = $3, response_body = $4, last_error = $5,
            next_retry_at = $6, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(d.id)
    .bind(status)
    .bind(status_code)
    .bind(response_body)
    .bind(error)
    .bind(next_retry)
    .execute(&mut *tx)
    .await?;

    let new_failures = d.failure_count + 1;
    if new_failures >= AUTO_PAUSE_FAILURES {
        sqlx::query(
            r#"
            UPDATE webhook_endpoint
            SET failure_count = $3, status = 'paused',
                disabled_reason = 'auto-paused after consecutive delivery failures',
                updated_at = now()
            WHERE id = $1 AND org_id = $2 AND status = 'active'
            "#,
        )
        .bind(d.endpoint_id)
        .bind(d.org_id)
        .bind(new_failures)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            r#"
            UPDATE webhook_endpoint
            SET failure_count = $3, updated_at = now()
            WHERE id = $1 AND org_id = $2
            "#,
        )
        .bind(d.endpoint_id)
        .bind(d.org_id)
        .bind(new_failures)
        .execute(&mut *tx)
        .await?;
    }

    let envelope = EventEnvelope::new(
        org,
        Context::Admin,
        "webhook",
        "failed",
        1,
        system_actor(),
        serde_json::json!({
            "webhook_id": d.endpoint_public_id,
            "delivery_id": PublicId::new(IdKind::WebhookDelivery, d.id).as_str(),
            "event_id": d.event_id,
            "attempt": d.attempt,
            "error": error,
            "permanent": dead,
        }),
    );
    insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| DispatchError::Outbox(e.to_string()))?;

    tx.commit().await?;
    Ok(())
}

/// Background poll loop for reliability (tests + NATS-less envs).
pub async fn run_poll_loop(
    pool: PgPool,
    decryptor: WebhookDecryptor,
    interval: Duration,
    opts: DispatchOptions,
) {
    let mut tick = tokio::time::interval(interval);
    loop {
        tick.tick().await;
        match dispatch_once(&pool, &decryptor, 25, opts).await {
            Ok(stats) if stats.claimed > 0 => {
                tracing::info!(
                    claimed = stats.claimed,
                    delivered = stats.delivered,
                    failed = stats.failed,
                    "webhook dispatch batch"
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "webhook dispatch failed"),
        }
    }
}

/// Optional NATS JetStream consumer `integration--outbound-webhooks`.
pub async fn run_nats_consumer(pool: PgPool, nats_url: &str) -> Result<(), anyhow::Error> {
    use futures::StreamExt;

    let client = async_nats::connect(nats_url).await?;
    let js = async_nats::jetstream::new(client);
    let stream = js
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: companyos_outbox::relay::STREAM_NAME.to_string(),
            subjects: vec!["companyos.>".to_string()],
            ..Default::default()
        })
        .await?;

    let consumer = stream
        .get_or_create_consumer(
            "integration--outbound-webhooks",
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("integration--outbound-webhooks".into()),
                filter_subject: "companyos.>".into(),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            },
        )
        .await?;

    let mut messages = consumer.messages().await?;
    while let Some(msg) = messages.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "nats consumer message error");
                continue;
            }
        };
        match serde_json::from_slice::<EventEnvelope>(&msg.payload) {
            Ok(envelope) => {
                // Skip webhook audit fan-out loops (delivered/failed would re-enqueue).
                if envelope.context == Context::Admin && envelope.aggregate == "webhook" {
                    let _ = msg.ack().await;
                    continue;
                }
                if let Err(e) = crate::enqueue::enqueue_event(&pool, &envelope).await {
                    tracing::warn!(error = %e, "enqueue from nats failed");
                    continue;
                }
                let _ = msg.ack().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "invalid EventEnvelope on nats");
                let _ = msg.ack().await;
            }
        }
    }
    Ok(())
}
