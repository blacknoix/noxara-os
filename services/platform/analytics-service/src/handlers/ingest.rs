//! Ingest EventEnvelope → fact_invoice_issued (ClickHouse or Postgres mirror).
//!
//! ADR-011: analytics derives from the event stream only — never direct OLTP reads.

use axum::extract::State;
use axum::Json;
use companyos_events::{Context, EventEnvelope};
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, Transaction};

use crate::state::AppState;
use crate::types::IngestResponse;

async fn set_ingest(tx: &mut Transaction<'_, Postgres>) -> Result<(), AppError> {
    sqlx::query("SELECT set_config('app.analytics_ingest', '1', true)")
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "ingest", e.to_string()))?;
    Ok(())
}

#[utoipa::path(
    post,
    path = "/api/v1/analytics/internal/ingest",
    responses((status = 200, body = IngestResponse)),
    tag = "analytics-internal"
)]
pub async fn ingest(
    State(state): State<AppState>,
    Json(envelope): Json<EventEnvelope>,
) -> Result<Json<IngestResponse>, AppError> {
    let request_id = envelope.event_id.to_string();

    if envelope.context != Context::Finance
        || envelope.aggregate != "invoice"
        || envelope.event_type != "issued"
    {
        return Ok(Json(IngestResponse {
            accepted: false,
            duplicate: false,
        }));
    }

    let invoice_id = envelope
        .payload
        .get("invoice_id")
        .or_else(|| envelope.payload.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let amount_minor = envelope
        .payload
        .get("amount_minor")
        .and_then(|v| v.as_i64());
    let currency = envelope
        .payload
        .get("currency")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(ch) = &state.clickhouse_url {
        let sql = format!(
            "INSERT INTO fact_invoice_issued (event_id, org_id, invoice_id, amount_minor, currency, issued_at) \
             VALUES ('{}', '{}', '{}', {}, '{}', '{}')",
            envelope.event_id,
            envelope.org_id.as_uuid(),
            invoice_id.replace('\'', ""),
            amount_minor
                .map(|n| n.to_string())
                .unwrap_or_else(|| "NULL".into()),
            currency.clone().unwrap_or_default().replace('\'', ""),
            envelope.occurred_at.to_rfc3339(),
        );
        let _ = state
            .http
            .post(format!("{}/", ch.trim_end_matches('/')))
            .query(&[("query", sql.as_str())])
            .send()
            .await;
        return Ok(Json(IngestResponse {
            accepted: true,
            duplicate: false,
        }));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_ingest(&mut tx).await?;
    set_session_org_id(&mut tx, envelope.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let existing: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT event_id FROM analytics_fact_invoice_issued WHERE event_id = $1")
            .bind(envelope.event_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    if existing.is_some() {
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
        return Ok(Json(IngestResponse {
            accepted: true,
            duplicate: true,
        }));
    }

    sqlx::query(
        r#"
        INSERT INTO analytics_fact_invoice_issued
            (event_id, org_id, invoice_id, amount_minor, currency, issued_at, ingested_at)
        VALUES ($1, $2, $3, $4, $5, $6, now())
        "#,
    )
    .bind(envelope.event_id)
    .bind(envelope.org_id.as_uuid())
    .bind(&invoice_id)
    .bind(amount_minor)
    .bind(currency.as_deref())
    .bind(envelope.occurred_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO analytics_cursor (consumer_name, last_event_id, last_occurred_at, updated_at)
        VALUES ('invoice_issued', $1, $2, now())
        ON CONFLICT (consumer_name) DO UPDATE SET
            last_event_id = EXCLUDED.last_event_id,
            last_occurred_at = EXCLUDED.last_occurred_at,
            updated_at = now()
        "#,
    )
    .bind(envelope.event_id)
    .bind(envelope.occurred_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(Json(IngestResponse {
        accepted: true,
        duplicate: false,
    }))
}
