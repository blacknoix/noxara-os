//! Transactional outbox: insert domain change + event in one DB transaction.
//!
//! Publishers poll `outbox_event` and push at-least-once to NATS JetStream.
//! Consumers must be idempotent via `idempotency_key`.
//!
//! Phase 1.8 adds a **relay** (`relay` module): claim unpublished rows with
//! `SKIP LOCKED`, publish, mark published, or move to `outbox_dlq`.
//!
//! Optional embedded MemoryPublisher loop: [`spawn::spawn_embedded_relay_if_configured`].
//! Production publishing is the dedicated `companyos-outbox-relay` binary.

pub mod relay;
pub mod spawn;

use chrono::{DateTime, Utc};
use companyos_events::EventEnvelope;
use companyos_tenancy::OrgId;
use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum OutboxError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("relay error: {0}")]
    Relay(String),
}

/// Row stored in `outbox_event`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, sqlx::FromRow)]
pub struct OutboxEvent {
    pub id: Uuid,
    pub org_id: Uuid,
    pub subject: String,
    pub payload: serde_json::Value,
    pub idempotency_key: String,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

/// Insert an envelope into the outbox using the caller's transaction/executor.
///
/// Must run in the **same transaction** as the domain write.
pub async fn insert_event<'e, E>(executor: E, envelope: &EventEnvelope) -> Result<Uuid, OutboxError>
where
    E: PgExecutor<'e>,
{
    let id = envelope.event_id;
    // Store full envelope as payload so consumers have actor + metadata.
    let wire = serde_json::to_value(envelope)
        .map_err(|e| OutboxError::Db(sqlx::Error::Protocol(format!("envelope serialize: {e}"))))?;
    sqlx::query(
        r#"
        INSERT INTO outbox_event (id, org_id, subject, payload, idempotency_key, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(envelope.org_id.as_uuid())
    .bind(&envelope.subject)
    .bind(wire)
    .bind(&envelope.idempotency_key)
    .bind(envelope.occurred_at)
    .execute(executor)
    .await?;
    Ok(id)
}

/// Fetch unpublished events for a publisher loop (at-least-once delivery).
pub async fn fetch_unpublished<'e, E>(
    executor: E,
    limit: i64,
) -> Result<Vec<OutboxEvent>, OutboxError>
where
    E: PgExecutor<'e>,
{
    let rows: Vec<OutboxEvent> = sqlx::query_as(
        r#"
        SELECT id, org_id, subject, payload, idempotency_key, created_at, published_at
        FROM outbox_event
        WHERE published_at IS NULL
        ORDER BY created_at ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

pub async fn mark_published<'e, E>(executor: E, id: Uuid) -> Result<(), OutboxError>
where
    E: PgExecutor<'e>,
{
    sqlx::query(
        r#"
        UPDATE outbox_event SET published_at = now() WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Helper to assert org_id is present on an envelope before insert.
pub fn assert_tenant_bound(envelope: &EventEnvelope) -> OrgId {
    envelope.org_id
}

/// Apply outbox migrations (001 + 002 relay/DLQ). Safe to call repeatedly.
pub async fn migrate(pool: &sqlx::PgPool) -> Result<(), OutboxError> {
    for sql in [
        include_str!("../migrations/001_outbox_event.sql"),
        include_str!("../migrations/002_outbox_relay.sql"),
    ] {
        for stmt in split_sql(sql) {
            sqlx::query(&stmt).execute(pool).await?;
        }
    }
    Ok(())
}

fn split_sql(sql: &str) -> Vec<String> {
    let cleaned: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    cleaned
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_events::{Context, EventEnvelope};
    use companyos_tenancy::{Actor, OrgId};

    #[test]
    fn insert_contract_requires_org_and_subject() {
        let org = OrgId::generate();
        let actor = Actor::human(companyos_ids::new_uuid_v7());
        let env = EventEnvelope::new(
            org,
            Context::Core,
            "hello",
            "created",
            1,
            actor,
            serde_json::json!({"ok": true}),
        );
        assert_eq!(assert_tenant_bound(&env), org);
        assert!(env.subject.contains(&org.to_public().as_str()));
        assert!(!env.idempotency_key.is_empty());
        assert_eq!(env.event_id.get_version_num(), 7);
    }

    #[test]
    fn envelope_serializes_for_outbox_payload() {
        let org = OrgId::generate();
        let env = EventEnvelope::new(
            org,
            Context::Core,
            "hello",
            "created",
            1,
            Actor::human(companyos_ids::new_uuid_v7()),
            serde_json::json!({"message": "hi"}),
        );
        let v = serde_json::to_value(&env).unwrap();
        assert!(v.get("org_id").is_some());
        assert!(v.get("subject").is_some());
        assert!(v.get("idempotency_key").is_some());
    }
}
