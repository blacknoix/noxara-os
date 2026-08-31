//! Event-stream consumer for raw events and governed typed facts.
//!
//! ADR-011: this consumer is the only writer of analytics facts. It never reads
//! or dual-writes from operational tables.

use axum::extract::State;
use axum::Json;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_tenancy::set_session_org_id;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};

use crate::metrics::FactSource;
use crate::state::AppState;
use crate::types::AnalyticsIngestResponse;

fn internal(request_id: &str) -> impl Fn(sqlx::Error) -> AppError + '_ {
    move |error| AppError::new(ErrorCode::Internal, request_id, error.to_string())
}

async fn set_ingest(tx: &mut Transaction<'_, Postgres>, request_id: &str) -> Result<(), AppError> {
    sqlx::query("SELECT set_config('app.analytics_ingest', '1', true)")
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    Ok(())
}

fn payload_text<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
}

fn payload_i64(payload: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_i64))
}

fn supported_fact(envelope: &EventEnvelope) -> Option<FactSource> {
    match (
        envelope.context,
        envelope.aggregate.as_str(),
        envelope.event_type.as_str(),
    ) {
        (Context::Finance, "invoice", "issued" | "voided" | "paid") => {
            Some(FactSource::InvoiceLifecycle)
        }
        (Context::Finance, "payment", "allocated" | "created") => Some(FactSource::Payment),
        (Context::Finance, "expense", "created" | "approved" | "paid") => Some(FactSource::Expense),
        (Context::Sales, "deal", "stage_changed" | "won" | "lost") => {
            Some(FactSource::DealStageChange)
        }
        (Context::Operations, "task", "created" | "completed" | "updated") => {
            Some(FactSource::TaskLifecycle)
        }
        (Context::Ai, "usage", "recorded") => Some(FactSource::AiUsage),
        _ => None,
    }
}

async fn insert_fact(
    tx: &mut Transaction<'_, Postgres>,
    envelope: &EventEnvelope,
    fact: FactSource,
    request_id: &str,
) -> Result<(), AppError> {
    let org = envelope.org_id.as_uuid();
    let payload = &envelope.payload;
    match fact {
        FactSource::InvoiceLifecycle => {
            let invoice_id = payload_text(payload, &["invoice_id", "id"]).unwrap_or("unknown");
            let amount = payload_i64(payload, &["amount_minor", "total_minor"]);
            let currency = payload_text(payload, &["currency"]);
            sqlx::query(
                "INSERT INTO analytics_fact_invoice_lifecycle \
                 (event_id, org_id, invoice_id, lifecycle_event, amount_minor, currency, occurred_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (event_id) DO NOTHING",
            )
            .bind(envelope.event_id)
            .bind(org)
            .bind(invoice_id)
            .bind(&envelope.event_type)
            .bind(amount)
            .bind(currency)
            .bind(envelope.occurred_at)
            .execute(&mut **tx)
            .await
            .map_err(internal(request_id))?;

            if envelope.event_type == "issued" {
                sqlx::query(
                    "INSERT INTO analytics_fact_invoice_issued \
                     (event_id, org_id, invoice_id, amount_minor, currency, issued_at) \
                     VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (event_id) DO NOTHING",
                )
                .bind(envelope.event_id)
                .bind(org)
                .bind(invoice_id)
                .bind(amount)
                .bind(currency)
                .bind(envelope.occurred_at)
                .execute(&mut **tx)
                .await
                .map_err(internal(request_id))?;
            }
        }
        FactSource::Payment => {
            sqlx::query(
                "INSERT INTO analytics_fact_payment \
                 (event_id, org_id, payment_id, invoice_id, amount_minor, currency, occurred_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (event_id) DO NOTHING",
            )
            .bind(envelope.event_id)
            .bind(org)
            .bind(payload_text(payload, &["payment_id", "id"]).unwrap_or("unknown"))
            .bind(payload_text(payload, &["invoice_id"]))
            .bind(payload_i64(payload, &["amount_minor", "total_minor"]))
            .bind(payload_text(payload, &["currency"]))
            .bind(envelope.occurred_at)
            .execute(&mut **tx)
            .await
            .map_err(internal(request_id))?;
        }
        FactSource::Expense => {
            sqlx::query(
                "INSERT INTO analytics_fact_expense \
                 (event_id, org_id, expense_id, lifecycle_event, amount_minor, currency, category, occurred_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (event_id) DO NOTHING",
            )
            .bind(envelope.event_id)
            .bind(org)
            .bind(payload_text(payload, &["expense_id", "id"]).unwrap_or("unknown"))
            .bind(&envelope.event_type)
            .bind(payload_i64(payload, &["amount_minor", "total_minor"]))
            .bind(payload_text(payload, &["currency"]))
            .bind(payload_text(payload, &["category", "category_id"]))
            .bind(envelope.occurred_at)
            .execute(&mut **tx)
            .await
            .map_err(internal(request_id))?;
        }
        FactSource::DealStageChange => {
            let to_stage =
                payload_text(payload, &["to_stage", "stage", "stage_id"]).or_else(|| {
                    matches!(envelope.event_type.as_str(), "won" | "lost")
                        .then_some(envelope.event_type.as_str())
                });
            sqlx::query(
                "INSERT INTO analytics_fact_deal_stage_change \
                 (event_id, org_id, deal_id, from_stage, to_stage, amount_minor, currency, occurred_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (event_id) DO NOTHING",
            )
            .bind(envelope.event_id)
            .bind(org)
            .bind(payload_text(payload, &["deal_id", "id"]).unwrap_or("unknown"))
            .bind(payload_text(payload, &["from_stage", "from_stage_id"]))
            .bind(to_stage)
            .bind(payload_i64(payload, &["amount_minor"]))
            .bind(payload_text(payload, &["currency"]))
            .bind(envelope.occurred_at)
            .execute(&mut **tx)
            .await
            .map_err(internal(request_id))?;
        }
        FactSource::TaskLifecycle => {
            sqlx::query(
                "INSERT INTO analytics_fact_task_lifecycle \
                 (event_id, org_id, task_id, lifecycle_event, project_id, status, occurred_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (event_id) DO NOTHING",
            )
            .bind(envelope.event_id)
            .bind(org)
            .bind(payload_text(payload, &["task_id", "id"]).unwrap_or("unknown"))
            .bind(&envelope.event_type)
            .bind(payload_text(payload, &["project_id"]))
            .bind(payload_text(payload, &["status"]))
            .bind(envelope.occurred_at)
            .execute(&mut **tx)
            .await
            .map_err(internal(request_id))?;
        }
        FactSource::AiUsage => {
            sqlx::query(
                "INSERT INTO analytics_fact_ai_usage \
                 (event_id, org_id, usage_kind, tokens, model, occurred_at) \
                 VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (event_id) DO NOTHING",
            )
            .bind(envelope.event_id)
            .bind(org)
            .bind(payload_text(payload, &["usage_kind", "kind"]).unwrap_or("unknown"))
            .bind(payload_i64(payload, &["tokens", "total_tokens"]))
            .bind(payload_text(payload, &["model"]))
            .bind(envelope.occurred_at)
            .execute(&mut **tx)
            .await
            .map_err(internal(request_id))?;
        }
        FactSource::ApiRequest | FactSource::InvoiceIssued => {}
    }
    Ok(())
}

fn clickhouse_row(envelope: &EventEnvelope, fact: FactSource) -> Value {
    let payload = &envelope.payload;
    let mut row = json!({
        "event_id": envelope.event_id,
        "org_id": envelope.org_id.as_uuid(),
        "occurred_at": envelope.occurred_at.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
    });
    let object = row.as_object_mut().expect("object");
    let fields: &[(&str, Value)] = match fact {
        FactSource::InvoiceLifecycle => &[
            (
                "invoice_id",
                json!(payload_text(payload, &["invoice_id", "id"]).unwrap_or("unknown")),
            ),
            ("lifecycle_event", json!(envelope.event_type)),
            (
                "amount_minor",
                json!(payload_i64(payload, &["amount_minor", "total_minor"]).unwrap_or(0)),
            ),
            (
                "currency",
                json!(payload_text(payload, &["currency"]).unwrap_or("")),
            ),
        ],
        FactSource::Payment => &[
            (
                "payment_id",
                json!(payload_text(payload, &["payment_id", "id"]).unwrap_or("unknown")),
            ),
            (
                "invoice_id",
                json!(payload_text(payload, &["invoice_id"]).unwrap_or("")),
            ),
            (
                "amount_minor",
                json!(payload_i64(payload, &["amount_minor", "total_minor"]).unwrap_or(0)),
            ),
            (
                "currency",
                json!(payload_text(payload, &["currency"]).unwrap_or("")),
            ),
        ],
        FactSource::Expense => &[
            (
                "expense_id",
                json!(payload_text(payload, &["expense_id", "id"]).unwrap_or("unknown")),
            ),
            ("lifecycle_event", json!(envelope.event_type)),
            (
                "amount_minor",
                json!(payload_i64(payload, &["amount_minor", "total_minor"]).unwrap_or(0)),
            ),
            (
                "currency",
                json!(payload_text(payload, &["currency"]).unwrap_or("")),
            ),
            (
                "category",
                json!(payload_text(payload, &["category", "category_id"]).unwrap_or("")),
            ),
        ],
        FactSource::DealStageChange => &[
            (
                "deal_id",
                json!(payload_text(payload, &["deal_id", "id"]).unwrap_or("unknown")),
            ),
            (
                "from_stage",
                json!(payload_text(payload, &["from_stage", "from_stage_id"]).unwrap_or("")),
            ),
            (
                "to_stage",
                json!(payload_text(payload, &["to_stage", "stage", "stage_id"])
                    .unwrap_or(&envelope.event_type)),
            ),
            (
                "amount_minor",
                json!(payload_i64(payload, &["amount_minor"]).unwrap_or(0)),
            ),
            (
                "currency",
                json!(payload_text(payload, &["currency"]).unwrap_or("")),
            ),
        ],
        FactSource::TaskLifecycle => &[
            (
                "task_id",
                json!(payload_text(payload, &["task_id", "id"]).unwrap_or("unknown")),
            ),
            ("lifecycle_event", json!(envelope.event_type)),
            (
                "project_id",
                json!(payload_text(payload, &["project_id"]).unwrap_or("")),
            ),
            (
                "status",
                json!(payload_text(payload, &["status"]).unwrap_or("")),
            ),
        ],
        FactSource::AiUsage => &[
            (
                "usage_kind",
                json!(payload_text(payload, &["usage_kind", "kind"]).unwrap_or("unknown")),
            ),
            (
                "tokens",
                json!(payload_i64(payload, &["tokens", "total_tokens"]).unwrap_or(0)),
            ),
            (
                "model",
                json!(payload_text(payload, &["model"]).unwrap_or("")),
            ),
        ],
        FactSource::ApiRequest | FactSource::InvoiceIssued => &[],
    };
    object.extend(
        fields
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone())),
    );
    row
}

async fn best_effort_clickhouse(state: &AppState, envelope: &EventEnvelope, fact: FactSource) {
    let Some(url) = &state.clickhouse_url else {
        return;
    };
    let query = format!("INSERT INTO {} FORMAT JSONEachRow", fact.as_str());
    let body = format!("{}\n", clickhouse_row(envelope, fact));
    if let Err(error) = state
        .http
        .post(format!("{}/", url.trim_end_matches('/')))
        .query(&[("query", query)])
        .body(body)
        .send()
        .await
    {
        tracing::warn!(%error, event_id = %envelope.event_id, "ClickHouse fact insert failed");
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/analytics/internal/ingest",
    responses((status = 200, body = AnalyticsIngestResponse)),
    tag = "analytics-internal"
)]
pub async fn ingest(
    State(state): State<AppState>,
    Json(envelope): Json<EventEnvelope>,
) -> Result<Json<AnalyticsIngestResponse>, AppError> {
    let request_id = envelope.event_id.to_string();
    let fact = supported_fact(&envelope);
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_ingest(&mut tx, &request_id).await?;
    set_session_org_id(&mut tx, envelope.org_id)
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, &request_id, error.to_string()))?;

    let inserted: Option<(uuid::Uuid,)> = sqlx::query_as(
        "INSERT INTO analytics_events_raw \
         (event_id, org_id, subject, context, aggregate, event_type, version, occurred_at, \
          actor_kind, actor_user_id, payload) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
         ON CONFLICT (event_id) DO NOTHING RETURNING event_id",
    )
    .bind(envelope.event_id)
    .bind(envelope.org_id.as_uuid())
    .bind(&envelope.subject)
    .bind(envelope.context.as_str())
    .bind(&envelope.aggregate)
    .bind(&envelope.event_type)
    .bind(i32::try_from(envelope.version).unwrap_or(i32::MAX))
    .bind(envelope.occurred_at)
    .bind(if envelope.actor.is_ai { "ai" } else { "human" })
    .bind(envelope.actor.user_id)
    .bind(&envelope.payload)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let duplicate = inserted.is_none();
    if !duplicate {
        if let Some(source) = fact {
            insert_fact(&mut tx, &envelope, source, &request_id).await?;
            sqlx::query(
                "INSERT INTO analytics_cursor \
                 (consumer_name, last_event_id, last_occurred_at, updated_at) \
                 VALUES ($1,$2,$3,now()) ON CONFLICT (consumer_name) DO UPDATE SET \
                 last_event_id = EXCLUDED.last_event_id, \
                 last_occurred_at = EXCLUDED.last_occurred_at, updated_at = now()",
            )
            .bind(source.consumer_name())
            .bind(envelope.event_id)
            .bind(envelope.occurred_at)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        }
        sqlx::query(
            "INSERT INTO analytics_freshness \
             (org_id, last_event_at, last_ingest_at, lag_seconds, updated_at) \
             VALUES ($1,$2,now(),GREATEST(EXTRACT(EPOCH FROM (now() - $2))::bigint,0),now()) \
             ON CONFLICT (org_id) DO UPDATE SET \
             last_event_at = GREATEST(analytics_freshness.last_event_at, EXCLUDED.last_event_at), \
             last_ingest_at = now(), \
             lag_seconds = GREATEST(EXTRACT(EPOCH FROM (now() - EXCLUDED.last_event_at))::bigint,0), \
             updated_at = now()",
        )
        .bind(envelope.org_id.as_uuid())
        .bind(envelope.occurred_at)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;

    if !duplicate {
        if let Some(source) = fact {
            best_effort_clickhouse(&state, &envelope, source).await;
        }
    }
    Ok(Json(AnalyticsIngestResponse {
        accepted: fact.is_some(),
        duplicate,
        fact: fact.map(|source| source.as_str().to_string()),
    }))
}
