use axum::extract::State;
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};

use crate::auth::AuthCtx;
use crate::metrics::{catalogue_golden_json, list_metrics};
use crate::state::AppState;
use crate::types::MetricListResponse;

use super::authorize;

#[utoipa::path(get, path = "/api/v1/analytics/metrics", tag = "analytics-metrics",
    responses((status = 200, body = MetricListResponse)))]
pub async fn metrics(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<MetricListResponse>, AppError> {
    authorize(&state, &auth, perms::analytics_report_read()).await?;
    Ok(Json(MetricListResponse {
        metrics: list_metrics(),
    }))
}

#[utoipa::path(get, path = "/api/v1/analytics/metrics/golden", tag = "analytics-metrics",
    responses((status = 200, body = [crate::metrics::MetricDefinition])))]
pub async fn golden(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&state, &auth, perms::analytics_report_read()).await?;
    let value = serde_json::from_str(&catalogue_golden_json()).map_err(|error| {
        AppError::new(ErrorCode::Internal, &auth.ctx.request_id, error.to_string())
    })?;
    Ok(Json(value))
}
