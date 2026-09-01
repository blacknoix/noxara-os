//! TRD 8.2 game day — NATS down → writes continue, outbox accumulates.

use companyos_events::{Context, EventEnvelope};
use companyos_ids::new_uuid_v7;
use companyos_outbox::{insert_event, migrate};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use std::sync::OnceLock;
use tokio::sync::Mutex;

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()
}

fn migrate_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn unpublished_count(pool: &sqlx::PgPool, org: OrgId) -> usize {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let n = companyos_outbox::fetch_unpublished(&mut *tx, 10_000)
        .await
        .unwrap()
        .len();
    tx.commit().await.unwrap();
    n
}

#[tokio::test]
async fn nats_down_writes_continue_outbox_accumulates() {
    let Some(url) = test_db_url() else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("db");
    {
        let _g = migrate_lock().lock().await;
        migrate(&pool).await.expect("migrate");
    }

    let org = OrgId::generate();
    let actor = Actor::human(new_uuid_v7());
    let before = unpublished_count(&pool, org).await;

    for i in 0..3 {
        let mut tx = pool.begin().await.unwrap();
        set_session_org_id(&mut tx, org).await.unwrap();
        let env = EventEnvelope::new(
            org,
            Context::Core,
            "hello",
            "created",
            1,
            actor.clone(),
            serde_json::json!({ "i": i, "nats": "down" }),
        );
        insert_event(&mut *tx, &env)
            .await
            .expect("domain write + outbox must succeed without NATS");
        tx.commit().await.unwrap();
    }

    let after = unpublished_count(&pool, org).await;
    assert!(
        after >= before + 3,
        "unpublished outbox must accumulate when NATS is down: before={before} after={after}"
    );
}
