use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use serde::Deserialize;

use crate::auth::AuthCtx;
use crate::state::AppState;
use crate::types::FreshnessResponse;

use super::{authorize, internal, set_org};

#[derive(Debug, Deserialize)]
pub struct FreshnessQuery {
    pub org_id: String,
}

#[utoipa::path(get, path = "/api/v1/analytics/freshness", tag = "analytics",
    params(("org_id" = String, Query)), responses((status = 200, body = FreshnessResponse)))]
pub async fn freshness(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(query): Query<FreshnessQuery>,
) -> Result<Json<FreshnessResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    if query.org_id != auth.ctx.org_id.to_public().as_str() {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "org_id does not match authenticated tenant",
        ));
    }
    authorize(&state, &auth, perms::analytics_report_read()).await?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row: Option<(Option<DateTime<Utc>>, Option<DateTime<Utc>>, i64)> = sqlx::query_as(
        "SELECT last_event_at, last_ingest_at, lag_seconds \
         FROM analytics_freshness WHERE org_id = $1",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    let (last_event_at, last_ingest_at, lag_seconds) = row.unwrap_or((None, None, 0));
    Ok(Json(FreshnessResponse {
        org_id: query.org_id,
        last_event_at,
        last_ingest_at,
        lag_seconds,
        eventually_consistent: true,
    }))
}
