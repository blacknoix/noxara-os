use axum::extract::{Query, State};
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use serde::Deserialize;

use crate::auth::AuthCtx;
use crate::metrics::{flagship_metrics, MeasureKind};
use crate::query::viewer_may_see_metric;
use crate::state::AppState;
use crate::types::{BenchmarkMetric, BenchmarkResponse};

use super::{authorize, internal, set_org};

#[derive(Debug, Deserialize)]
pub struct BenchmarkQuery {
    pub org_id: String,
}

#[utoipa::path(get, path = "/api/v1/analytics/benchmarks", tag = "analytics",
    params(("org_id" = String, Query)), responses((status = 200, body = BenchmarkResponse)))]
pub async fn benchmarks(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(query): Query<BenchmarkQuery>,
) -> Result<Json<BenchmarkResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    if query.org_id != auth.ctx.org_id.to_public().as_str() {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "org_id does not match authenticated tenant",
        ));
    }
    let principal = authorize(&state, &auth, perms::analytics_report_read()).await?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let mut values = Vec::new();
    for metric in flagship_metrics() {
        if principal
            .as_ref()
            .is_some_and(|value| !viewer_may_see_metric(value, &metric))
        {
            continue;
        }
        let value_expr = match metric.measure {
            MeasureKind::Count => "1".to_string(),
            MeasureKind::Sum | MeasureKind::Avg => metric.measure_field.clone(),
        };
        let filter = match metric.name.as_str() {
            "revenue_issued" => " AND lifecycle_event = 'issued'",
            _ => "",
        };
        let sql = format!(
            "SELECT \
             COALESCE(SUM(CASE WHEN occurred_at >= now() - INTERVAL '7 days' \
             THEN {value_expr} ELSE 0 END),0)::bigint, \
             COALESCE(SUM(CASE WHEN occurred_at >= now() - INTERVAL '14 days' \
             AND occurred_at < now() - INTERVAL '7 days' \
             THEN {value_expr} ELSE 0 END),0)::bigint \
             FROM {} WHERE org_id = $1{filter}",
            metric.fact.postgres_table()
        );
        let (current_value, previous_value): (i64, i64) = sqlx::query_as(&sql)
            .bind(auth.ctx.org_id.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(internal(request_id))?;
        let trend_percent = if previous_value == 0 {
            None
        } else {
            Some(((current_value - previous_value) as f64 / previous_value.abs() as f64) * 100.0)
        };
        values.push(BenchmarkMetric {
            metric: metric.name,
            display_name: metric.display_name,
            unit: metric.unit,
            current_value,
            previous_value,
            trend_percent,
        });
    }
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(BenchmarkResponse {
        org_id: query.org_id,
        window_days: 7,
        benchmarks: values,
    }))
}
