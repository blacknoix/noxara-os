//! Integration tests for the core hello vertical slice.
//! Requires TEST_DATABASE_URL / DATABASE_URL.

use companyos_events::EventEnvelope;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use companyos_testkit::{connect, test_database_url};
use sqlx::Row;

async fn apply_core_migrations(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    let sql = include_str!("../migrations/001_init.sql");
    for stmt in sql.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        if stmt
            .lines()
            .all(|l| l.trim().is_empty() || l.trim_start().starts_with("--"))
        {
            continue;
        }
        sqlx::query(stmt).execute(pool).await?;
    }
    Ok(())
}

#[tokio::test]
async fn hello_write_outbox_and_cross_tenant_isolation() {
    if test_database_url().is_none() {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    }
    let pool = connect().await.expect("db");
    apply_core_migrations(&pool).await.expect("migrate");

    let org_a = OrgId::generate();
    let org_b = OrgId::generate();
    let user_a = new_uuid_v7();
    let user_b = new_uuid_v7();

    for (org, name) in [(org_a, "Acme A"), (org_b, "Acme B")] {
        sqlx::query(
            "INSERT INTO organization (id, public_id, name) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(org.as_uuid())
        .bind(org.to_public().as_str())
        .bind(name)
        .execute(&pool)
        .await
        .expect("org");
    }

    let hello_id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Hello, hello_id);
    let actor = Actor::human(user_a);
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org_a).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO hello_message (id, org_id, public_id, message, created_by)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(hello_id)
    .bind(org_a.as_uuid())
    .bind(public_id.as_str())
    .bind("hello A")
    .bind(user_a)
    .execute(&mut *tx)
    .await
    .unwrap();

    let envelope = companyos_events::EventEnvelope::new(
        org_a,
        companyos_events::Context::Core,
        "hello",
        "created",
        1,
        actor,
        serde_json::json!({ "id": public_id.as_str(), "message": "hello A" }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // Test consumer sees the event in outbox for org A (must be inside a TX for RLS).
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org_a).await.unwrap();
    let events = companyos_outbox::fetch_unpublished(&mut *tx, 50)
        .await
        .unwrap();
    assert!(
        events.iter().any(|e| e.subject == envelope.subject),
        "consumer must see outbox event {}; saw {:?}",
        envelope.subject,
        events.iter().map(|e| &e.subject).collect::<Vec<_>>()
    );
    let matched = events
        .iter()
        .find(|e| e.subject == envelope.subject)
        .unwrap();
    let wire: EventEnvelope = serde_json::from_value(matched.payload.clone()).unwrap();
    assert_eq!(wire.org_id, org_a);
    tx.commit().await.unwrap();

    // Org B cannot read org A's hello.
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org_b).await.unwrap();
    let found: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM hello_message WHERE id = $1")
            .bind(hello_id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
    assert!(found.is_none(), "org B must not read org A hello");

    let rows = sqlx::query("SELECT id, org_id FROM hello_message")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    for row in rows {
        let org: uuid::Uuid = row.try_get("org_id").unwrap();
        assert_ne!(org, org_a.as_uuid(), "planted SELECT leaked org A to org B");
    }
    tx.commit().await.unwrap();

    let _ = user_b;
}
