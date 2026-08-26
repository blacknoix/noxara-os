//! Finance customer projection from Sales CRM events.
//!
//! Sales owns the customer aggregate (ADR 009). Finance never reads `sales_*`
//! tables. The same handler is used for NATS/outbox relay consumption and
//! in-process test application.

use companyos_events::EventEnvelope;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::OrgId;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct CustomerCreatedPayload {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DealWonPayload {
    pub id: String,
    #[serde(default)]
    pub amount_minor: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuoteAcceptedPayload {
    pub id: String,
    #[serde(default)]
    pub total_minor: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
}

/// Apply a CRM/Sales event envelope to finance projections.
/// Returns true if the event was handled (or intentionally ignored).
pub async fn apply_sales_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    envelope: &EventEnvelope,
) -> Result<bool, sqlx::Error> {
    if envelope.context.as_str() != "sales" {
        return Ok(false);
    }
    match (envelope.aggregate.as_str(), envelope.event_type.as_str()) {
        ("customer", "created") => {
            let payload: CustomerCreatedPayload =
                serde_json::from_value(envelope.payload.clone())
                    .map_err(|e| sqlx::Error::Protocol(format!("bad customer.created: {e}")))?;
            upsert_customer_projection(tx, envelope.org_id, &payload).await?;
            Ok(true)
        }
        ("deal", "won") | ("quote", "accepted") => {
            // Informational for finance; customer must already be projected.
            // Stored as no-op for projection — invoice creation is user-driven
            // (quote snapshot passed via API).
            let _ = (
                serde_json::from_value::<DealWonPayload>(envelope.payload.clone()),
                serde_json::from_value::<QuoteAcceptedPayload>(envelope.payload.clone()),
            );
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub async fn upsert_customer_projection(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: OrgId,
    payload: &CustomerCreatedPayload,
) -> Result<Uuid, sqlx::Error> {
    let sales_pid: PublicId = payload
        .id
        .parse()
        .map_err(|e| sqlx::Error::Protocol(format!("bad customer id: {e}")))?;
    if sales_pid.kind() != IdKind::Customer {
        return Err(sqlx::Error::Protocol("expected cus_ public id".into()));
    }

    // Idempotent: same sales customer → same finance projection row.
    let existing: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT id FROM finance_customer
        WHERE org_id = $1 AND sales_customer_public_id = $2
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(&payload.id)
    .fetch_optional(&mut **tx)
    .await?;

    if let Some((id,)) = existing {
        sqlx::query(
            r#"
            UPDATE finance_customer
            SET name = $2, email = COALESCE($3, email), updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(&payload.name)
        .bind(payload.email.as_deref())
        .execute(&mut **tx)
        .await?;
        return Ok(id);
    }

    let id = new_uuid_v7();
    // Finance projection keeps the same public cus_ id for BFF correlation,
    // but stores it under finance_customer.public_id as well.
    let currency = payload
        .currency
        .clone()
        .unwrap_or_else(|| "USD".to_string());
    sqlx::query(
        r#"
        INSERT INTO finance_customer (
            id, org_id, public_id, sales_customer_public_id, name, email, currency
        ) VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(&payload.id)
    .bind(&payload.id)
    .bind(&payload.name)
    .bind(payload.email.as_deref())
    .bind(&currency)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

/// Ensure a finance customer exists for a sales customer public id, creating
/// a stub projection when only the id+name are known (e.g. quote→invoice API
/// that passes a snapshot without a prior event).
pub async fn ensure_customer_from_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: OrgId,
    sales_customer_public_id: &str,
    name: &str,
    currency: &str,
) -> Result<Uuid, sqlx::Error> {
    let payload = CustomerCreatedPayload {
        id: sales_customer_public_id.to_string(),
        name: name.to_string(),
        email: None,
        currency: Some(currency.to_string()),
    };
    upsert_customer_projection(tx, org_id, &payload).await
}
