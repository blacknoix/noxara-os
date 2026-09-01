//! Event ingest → upsert search document.

use axum::extract::State;
use axum::Json;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::EventEnvelope;
use companyos_ids::new_uuid_v7;
use tracing::warn;

use crate::mapping::{doc_type_from_aggregate, doc_type_from_custom_aggregate};
use crate::state::{AppState, SearchDoc};
use crate::types::IngestResponse;
use companyos_events::Context;

#[utoipa::path(
    post,
    path = "/api/v1/search/internal/ingest",
    responses((status = 200, body = IngestResponse)),
    tag = "search-internal"
)]
pub async fn ingest(
    State(state): State<AppState>,
    Json(envelope): Json<EventEnvelope>,
) -> Result<Json<IngestResponse>, AppError> {
    let doc_type = if envelope.context == Context::Custom {
        Some(doc_type_from_custom_aggregate(&envelope.aggregate))
    } else {
        doc_type_from_aggregate(&envelope.aggregate).map(|s| s.to_string())
    };
    let Some(doc_type) = doc_type else {
        return Ok(Json(IngestResponse { upserted: false }));
    };

    let doc_id = envelope
        .payload
        .get("record_id")
        .or_else(|| envelope.payload.get(format!("{}_id", envelope.aggregate)))
        .or_else(|| envelope.payload.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("doc_{}", new_uuid_v7().simple()));

    let title = envelope
        .payload
        .get("search_text")
        .or_else(|| envelope.payload.get("name"))
        .or_else(|| envelope.payload.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or(doc_type.as_str())
        .to_string();
    let body = envelope
        .payload
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let href = envelope
        .payload
        .get("href")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let doc = SearchDoc {
        org_id: envelope.org_id.as_uuid(),
        doc_id: doc_id.clone(),
        doc_type: doc_type.clone(),
        title,
        body,
        href,
    };

    // Always mirror to Postgres for OpenSearch-down fallback (game day / TRD 8.2).
    upsert_mirror(&state, &doc).await?;

    if let Some(base) = &state.opensearch_url {
        let url = format!(
            "{}/companyos/_doc/{}-{}",
            base.trim_end_matches('/'),
            envelope.org_id.as_uuid(),
            doc_id
        );
        match state.http.put(&url).json(&doc).send().await {
            Ok(resp) => {
                if let Err(e) = resp.error_for_status() {
                    warn!(error = %e, "OpenSearch ingest failed; Postgres mirror retained");
                }
            }
            Err(e) => {
                warn!(error = %e, "OpenSearch unreachable on ingest; Postgres mirror retained");
            }
        }
    } else {
        let mut map = state
            .memory
            .lock()
            .map_err(|e| AppError::new(ErrorCode::Internal, "search", e.to_string()))?;
        map.insert((doc.org_id, doc.doc_id.clone()), doc);
    }

    Ok(Json(IngestResponse { upserted: true }))
}

async fn upsert_mirror(state: &AppState, doc: &SearchDoc) -> Result<(), AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "search", e.to_string()))?;
    sqlx::query("SELECT set_config('app.search_ingest', '1', true)")
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "search", e.to_string()))?;
    sqlx::query(
        r#"
        INSERT INTO search_doc_mirror (org_id, doc_id, doc_type, title, body, href, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, now())
        ON CONFLICT (org_id, doc_id) DO UPDATE SET
            doc_type = EXCLUDED.doc_type,
            title = EXCLUDED.title,
            body = EXCLUDED.body,
            href = EXCLUDED.href,
            updated_at = now()
        "#,
    )
    .bind(doc.org_id)
    .bind(&doc.doc_id)
    .bind(&doc.doc_type)
    .bind(&doc.title)
    .bind(&doc.body)
    .bind(&doc.href)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, "search", e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "search", e.to_string()))?;
    Ok(())
}
