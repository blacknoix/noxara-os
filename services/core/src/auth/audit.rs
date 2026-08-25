//! Auth audit trail (every auth event).

use companyos_ids::new_uuid_v7;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn record(
    pool: &PgPool,
    org_id: Option<Uuid>,
    user_id: Option<Uuid>,
    event_type: &str,
    ip: Option<&str>,
    user_agent: Option<&str>,
    metadata: Value,
) {
    let id = new_uuid_v7();
    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO auth_audit_event (id, org_id, user_id, event_type, ip_address, user_agent, metadata)
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(user_id)
    .bind(event_type)
    .bind(ip)
    .bind(user_agent)
    .bind(metadata)
    .execute(pool)
    .await
    {
        tracing::error!(error = %e, event_type, "failed to write auth audit event");
    }
}
