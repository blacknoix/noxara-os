//! Nightly reconcile: mirror count vs expected fixture count (ADR-011 CI check).

use axum::extract::State;
use axum::Json;
use companyos_errors::{AppError, ErrorCode};
use serde::Deserialize;

use crate::auth::AuthCtx;
use crate::state::AppState;
use crate::types::ReconcileResponse;

#[derive(Debug, Deserialize)]
pub struct ReconcileBody {
    /// Expected event-derived fact count for the fixture (tests pass this).
    pub expected_count: i64,
    pub org_id: Option<uuid::Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/v1/analytics/reconcile/nightly",
    responses((status = 200, body = ReconcileResponse)),
    tag = "analytics"
)]
pub async fn nightly(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ReconcileBody>,
) -> Result<Json<ReconcileResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org = body.org_id.unwrap_or_else(|| auth.ctx.org_id.as_uuid());

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    sqlx::query("SELECT set_config('app.analytics_ingest', '1', true)")
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let (mirror_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM analytics_fact_invoice_issued WHERE org_id = $1",
    )
    .bind(org)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(Json(ReconcileResponse {
        mirror_count,
        expected_count: body.expected_count,
        matched: mirror_count == body.expected_count,
    }))
}
