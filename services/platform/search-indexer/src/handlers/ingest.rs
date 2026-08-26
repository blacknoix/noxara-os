//! Event ingest → upsert search document.

use axum::extract::State;
use axum::Json;
use companyos_events::EventEnvelope;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::new_uuid_v7;

use crate::mapping::doc_type_from_aggregate;
use crate::state::{AppState, SearchDoc};
use crate::types::IngestResponse;

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
    let Some(doc_type) = doc_type_from_aggregate(&envelope.aggregate) else {
        return Ok(Json(IngestResponse { upserted: false }));
    };

    let doc_id = envelope
        .payload
        .get(format!("{}_id", envelope.aggregate))
        .or_else(|| envelope.payload.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("doc_{}", new_uuid_v7().simple()));

    let title = envelope
        .payload
        .get("name")
        .or_else(|| envelope.payload.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or(doc_type)
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
        doc_type: doc_type.to_string(),
        title,
        body,
        href,
    };

    if let Some(base) = &state.opensearch_url {
        let url = format!(
            "{}/companyos/_doc/{}-{}",
            base.trim_end_matches('/'),
            envelope.org_id.as_uuid(),
            doc_id
        );
        state
            .http
            .put(&url)
            .json(&doc)
            .send()
            .await
            .map_err(|e| AppError::new(ErrorCode::ServiceUnavailable, "search", e.to_string()))?
            .error_for_status()
            .map_err(|e| AppError::new(ErrorCode::ServiceUnavailable, "search", e.to_string()))?;
    } else {
        let mut map = state
            .memory
            .lock()
            .map_err(|e| AppError::new(ErrorCode::Internal, "search", e.to_string()))?;
        map.insert((doc.org_id, doc.doc_id.clone()), doc);
    }

    Ok(Json(IngestResponse { upserted: true }))
}
