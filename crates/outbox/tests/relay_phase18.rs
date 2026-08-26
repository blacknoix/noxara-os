//! Phase 1.8 outbox relay integration tests.
//!
//! Skips when `TEST_DATABASE_URL` / `DATABASE_URL` is unset.

use std::sync::Arc;

use companyos_events::{Context, EventEnvelope};
use companyos_outbox::relay::{self, MemoryPublisher, RelayMetrics};
use companyos_outbox::{insert_event, migrate};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use uuid::Uuid;

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()
}

async fn connect() -> Option<sqlx::PgPool> {
    let url = test_db_url()?;
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()
}

#[tokio::test]
async fn relay_once_publishes_with_memory_publisher() {
    let Some(pool) = connect().await else {
        eprintln!("skip relay_once_publishes — no TEST_DATABASE_URL");
        return;
    };
    migrate(&pool).await.expect("migrate");

    let org = OrgId::generate();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let env = EventEnvelope::new(
        org,
        Context::Core,
        "hello",
        "created",
        1,
        Actor::human(companyos_ids::new_uuid_v7()),
        serde_json::json!({"hello_id": "hel_x", "message": "relay"}),
    );
    let idem = env.idempotency_key.clone();
    insert_event(&mut *tx, &env).await.unwrap();
    tx.commit().await.unwrap();

    let publisher = MemoryPublisher::new();
    let metrics = RelayMetrics::default();
    let n = relay::relay_once(&pool, &publisher, &metrics, 50, 1000)
        .await
        .unwrap();
    assert!(n >= 1, "expected at least one publish");
    assert!(publisher.delivered_count() >= 1);

    // Consumer idempotency: simulate consumer table keyed by idempotency_key.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS outbox_consumer_processed_test (
            idempotency_key TEXT PRIMARY KEY,
            processed_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let inserted = sqlx::query(
        "INSERT INTO outbox_consumer_processed_test (idempotency_key) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(&idem)
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(inserted, 1);

    let dup = sqlx::query(
        "INSERT INTO outbox_consumer_processed_test (idempotency_key) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(&idem)
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(dup, 0, "duplicate idempotency_key must be ignored");
}

#[tokio::test]
async fn force_fail_moves_to_dlq_then_replay() {
    let Some(pool) = connect().await else {
        eprintln!("skip force_fail_moves_to_dlq_then_replay — no TEST_DATABASE_URL");
        return;
    };
    migrate(&pool).await.expect("migrate");

    let org = OrgId::generate();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let env = EventEnvelope::new(
        org,
        Context::Core,
        "hello",
        "created",
        1,
        Actor::human(companyos_ids::new_uuid_v7()),
        serde_json::json!({"hello_id": "hel_dlq", "message": "fail"}),
    );
    insert_event(&mut *tx, &env).await.unwrap();
    tx.commit().await.unwrap();

    let publisher = MemoryPublisher::new();
    // Fail enough times that max_attempts_before_dlq (3) is exhausted.
    publisher.set_fail_times(10);
    let metrics = RelayMetrics::default();
    let _ = relay::relay_once(&pool, &publisher, &metrics, 50, 1000)
        .await
        .unwrap();
    assert!(metrics.snapshot().dlq >= 1, "expected DLQ increment");

    let rows = relay::list_unreplayed_dlq(&pool, 100).await.unwrap();
    assert!(!rows.is_empty(), "expected DLQ row");
    let dlq_id = rows[0].id;

    let new_outbox_id = relay::replay_dlq_row(&pool, dlq_id).await.unwrap();
    assert_ne!(new_outbox_id, Uuid::nil());

    // Clear fail flag and republish.
    publisher.set_fail_times(0);
    let before = publisher.delivered_count();
    let n = relay::relay_once(&pool, &publisher, &metrics, 50, 1000)
        .await
        .unwrap();
    assert!(n >= 1);
    assert!(publisher.delivered_count() > before);
}

#[tokio::test]
async fn spawn_helper_respects_env_flag() {
    // Unit-ish: helper returns false when env unset.
    std::env::remove_var("OUTBOX_EMBEDDED_RELAY");
    let Some(pool) = connect().await else {
        return;
    };
    assert!(!companyos_outbox::spawn::spawn_embedded_relay_if_configured(
        pool
    ));
    let _: Arc<dyn relay::EventPublisher> = Arc::new(MemoryPublisher::new());
}
