use axum::extract::State;
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};

use crate::auth::AuthCtx;
use crate::forecast::{
    forecast_from_history, map_series_to_metric, ForecastMethod, ForecastRequest, ForecastResponse,
};
use crate::metrics::{get_metric, MeasureKind, MetricUnit};
use crate::query::viewer_may_see_metric;
use crate::state::AppState;

use super::{authorize, internal, set_org};

#[utoipa::path(post, path = "/api/v1/analytics/forecasts", tag = "analytics-forecasts",
    request_body = ForecastRequest, responses((status = 200, body = ForecastResponse)))]
pub async fn forecast(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ForecastRequest>,
) -> Result<Json<ForecastResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    if body.org_id != auth.ctx.org_id.to_public().as_str() {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "forecast org_id does not match authenticated tenant",
        ));
    }
    if body.horizon_periods == 0 || body.horizon_periods > 52 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "horizon_periods must be between 1 and 52",
        ));
    }
    if body.history_periods == 0 || body.history_periods > 365 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "history_periods must be between 1 and 365",
        ));
    }
    let principal = authorize(&state, &auth, perms::analytics_report_run()).await?;
    let metric_name = map_series_to_metric(&body.series).ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("unknown forecast series '{}'", body.series),
        )
    })?;
    let metric = get_metric(metric_name).ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            request_id,
            "forecast metric is missing",
        )
    })?;
    if principal
        .as_ref()
        .is_some_and(|value| !viewer_may_see_metric(value, &metric))
    {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!("missing permission {}", metric.required_permission),
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let value_column = match metric.unit {
        MetricUnit::Count => "value_count",
        MetricUnit::MoneyMinor | MetricUnit::Tokens => "value_minor",
    };
    let rollup_sql = format!(
        "SELECT {value_column} FROM analytics_rollup_daily \
         WHERE org_id = $1 AND metric_name = $2 \
         ORDER BY day DESC LIMIT $3"
    );
    let mut history: Vec<i64> = sqlx::query_as::<_, (i64,)>(&rollup_sql)
        .bind(auth.ctx.org_id.as_uuid())
        .bind(metric_name)
        .bind(i64::from(body.history_periods))
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(request_id))?
        .into_iter()
        .map(|(value,)| value)
        .collect();
    history.reverse();

    if history.is_empty() {
        let aggregate = match metric.measure {
            MeasureKind::Sum => format!("COALESCE(SUM({}), 0)::bigint", metric.measure_field),
            MeasureKind::Count => "COUNT(*)::bigint".into(),
            MeasureKind::Avg => {
                format!("COALESCE(AVG({}), 0)::bigint", metric.measure_field)
            }
        };
        let lifecycle_filter = match metric.name.as_str() {
            "revenue_issued" => " AND lifecycle_event = 'issued'",
            "task_completions" => " AND lifecycle_event = 'completed'",
            _ => "",
        };
        let facts_sql = format!(
            "SELECT {aggregate} AS value FROM {} \
             WHERE org_id = $1 AND occurred_at >= \
             (CURRENT_DATE - ($2::int * INTERVAL '1 day')) {lifecycle_filter} \
             GROUP BY occurred_at::date ORDER BY occurred_at::date",
            metric.fact.postgres_table()
        );
        history = sqlx::query_as::<_, (i64,)>(&facts_sql)
            .bind(auth.ctx.org_id.as_uuid())
            .bind(i32::try_from(body.history_periods).unwrap_or(i32::MAX))
            .fetch_all(&mut *tx)
            .await
            .map_err(internal(request_id))?
            .into_iter()
            .map(|(value,)| value)
            .collect();
    }
    tx.commit().await.map_err(internal(request_id))?;

    let method = body.method.unwrap_or(ForecastMethod::TrailingAverage);
    let response = forecast_from_history(&body.series, &history, body.horizon_periods, method)
        .map_err(|error| AppError::new(ErrorCode::ValidationFailed, request_id, error))?;
    Ok(Json(response))
}
