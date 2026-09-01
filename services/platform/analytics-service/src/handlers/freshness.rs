use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use serde::Deserialize;
use tracing::warn;

use crate::auth::AuthCtx;
use crate::state::AppState;
use crate::types::FreshnessResponse;

use super::{authorize, internal, set_org};

#[derive(Debug, Deserialize)]
pub struct FreshnessQuery {
    pub org_id: String,
}

type FreshnessRow = (Option<DateTime<Utc>>, Option<DateTime<Utc>>, i64);

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
    let row: Option<FreshnessRow> = sqlx::query_as(
        "SELECT last_event_at, last_ingest_at, lag_seconds \
         FROM analytics_freshness WHERE org_id = $1",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    let (last_event_at, last_ingest_at, lag_seconds) = row.unwrap_or((None, None, 0));
    let clickhouse_degraded = probe_clickhouse_degraded(&state).await;
    Ok(Json(FreshnessResponse {
        org_id: query.org_id,
        last_event_at,
        last_ingest_at,
        lag_seconds,
        eventually_consistent: true,
        backend: "postgres_mirror".into(),
        clickhouse_degraded,
    }))
}

/// When CLICKHOUSE_URL is unset, CH is not in the path (not degraded).
/// When set, a failed probe marks dashboards as serving stale/mirror-only data.
pub async fn probe_clickhouse_degraded(state: &AppState) -> bool {
    let Some(url) = &state.clickhouse_url else {
        return false;
    };
    if std::env::var("CLICKHOUSE_FORCE_DOWN").ok().as_deref() == Some("1") {
        return true;
    }
    let probe = format!("{}/ping", url.trim_end_matches('/'));
    match state.http.get(&probe).send().await {
        Ok(resp) if resp.status().is_success() => false,
        Ok(resp) => {
            warn!(status = %resp.status(), "ClickHouse ping non-success; mirror-only");
            true
        }
        Err(e) => {
            warn!(error = %e, "ClickHouse unreachable; serving Postgres mirror");
            true
        }
    }
}
