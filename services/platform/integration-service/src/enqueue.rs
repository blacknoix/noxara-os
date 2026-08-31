//! Enqueue webhook deliveries from a domain [`EventEnvelope`].
//!
//! Finds active endpoints whose `event_types` subscription matches the event,
//! then inserts delivery rows with `ON CONFLICT DO NOTHING` (idempotent
//! `(org_id, endpoint_id, event_id)`).

use companyos_events::EventEnvelope;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum EnqueueError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("tenancy: {0}")]
    Tenancy(String),
}

#[derive(Debug, Clone, Default)]
pub struct EnqueueResult {
    pub matched_endpoints: usize,
    pub inserted: usize,
}

/// Canonical subscription key: `{context}.{aggregate}.{event_type}.v{version}`.
pub fn event_subscription_key(envelope: &EventEnvelope) -> String {
    format!(
        "{}.{}.{}.v{}",
        envelope.context.as_str(),
        envelope.aggregate,
        envelope.event_type,
        envelope.version
    )
}

/// True when an endpoint's `event_types` entry matches this envelope.
pub fn matches_subscription(sub: &str, envelope: &EventEnvelope) -> bool {
    let sub = sub.trim();
    if sub.is_empty() {
        return false;
    }
    if sub == "*" {
        return true;
    }
    if sub == envelope.subject {
        return true;
    }
    let key = event_subscription_key(envelope);
    if sub == key {
        return true;
    }
    // Allow versionless: `sales.deal.won`
    let versionless = format!(
        "{}.{}.{}",
        envelope.context.as_str(),
        envelope.aggregate,
        envelope.event_type
    );
    sub == versionless
}

async fn set_dispatch_session(tx: &mut Transaction<'_, Postgres>) -> Result<(), EnqueueError> {
    sqlx::query("SELECT set_config('app.webhook_dispatch', '1', true)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Insert pending delivery rows for all matching active endpoints in `envelope.org_id`.
pub async fn enqueue_event(
    pool: &PgPool,
    envelope: &EventEnvelope,
) -> Result<EnqueueResult, EnqueueError> {
    let mut tx = pool.begin().await?;
    set_dispatch_session(&mut tx).await?;
    set_session_org_id(&mut tx, envelope.org_id)
        .await
        .map_err(|e| EnqueueError::Tenancy(e.to_string()))?;

    let result = enqueue_event_tx(&mut tx, envelope).await?;
    tx.commit().await?;
    Ok(result)
}

/// Same as [`enqueue_event`] but using an open transaction (org session already set optional).
pub async fn enqueue_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    envelope: &EventEnvelope,
) -> Result<EnqueueResult, EnqueueError> {
    let rows: Vec<(Uuid, Value)> = sqlx::query_as(
        r#"
        SELECT id, event_types
        FROM webhook_endpoint
        WHERE org_id = $1 AND status = 'active'
        "#,
    )
    .bind(envelope.org_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    let mut matched = 0usize;
    let mut inserted = 0usize;
    let payload = serde_json::to_value(envelope).unwrap_or_else(|_| envelope.payload.clone());
    let event_type_key = event_subscription_key(envelope);

    for (endpoint_id, event_types_json) in rows {
        let types: Vec<String> = serde_json::from_value(event_types_json).unwrap_or_default();
        if !types.iter().any(|t| matches_subscription(t, envelope)) {
            continue;
        }
        matched += 1;

        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::WebhookDelivery, id);
        let res = sqlx::query(
            r#"
            INSERT INTO webhook_delivery (
                id, org_id, public_id, endpoint_id, event_id,
                event_subject, event_type, payload, attempt, status, next_retry_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,0,'pending',now())
            ON CONFLICT (org_id, endpoint_id, event_id) DO NOTHING
            "#,
        )
        .bind(id)
        .bind(envelope.org_id.as_uuid())
        .bind(public_id.as_str())
        .bind(endpoint_id)
        .bind(envelope.event_id)
        .bind(&envelope.subject)
        .bind(&event_type_key)
        .bind(&payload)
        .execute(&mut **tx)
        .await?;

        if res.rows_affected() > 0 {
            inserted += 1;
        }
    }

    Ok(EnqueueResult {
        matched_endpoints: matched,
        inserted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_events::Context;
    use companyos_tenancy::{Actor, OrgId};

    fn sample(agg: &str, ev: &str) -> EventEnvelope {
        EventEnvelope::new(
            OrgId::generate(),
            Context::Sales,
            agg,
            ev,
            1,
            Actor::human(Uuid::nil()),
            serde_json::json!({"deal_id": "dea_test"}),
        )
    }

    #[test]
    fn subscription_matching() {
        let env = sample("deal", "won");
        assert!(matches_subscription("*", &env));
        assert!(matches_subscription("sales.deal.won.v1", &env));
        assert!(matches_subscription("sales.deal.won", &env));
        assert!(matches_subscription(&env.subject, &env));
        assert!(!matches_subscription("sales.deal.lost.v1", &env));
        assert!(!matches_subscription("", &env));
    }
}
