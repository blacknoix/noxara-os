//! Governed forecasting — simple methods with full explainability.
//!
//! v1: linear trend / trailing average. Every response exposes method + inputs.
//! AI must not commit forecasts as the publishing actor.

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::metrics::{get_metric, MetricUnit};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ForecastMethod {
    /// Average of the last N periods, projected forward.
    TrailingAverage,
    /// Ordinary least-squares slope on the last N periods.
    LinearTrend,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ForecastRequest {
    pub org_id: String,
    /// One of: revenue, cash_flow, pipeline, headcount (maps to governed metrics).
    pub series: String,
    #[serde(default = "default_horizon")]
    pub horizon_periods: u32,
    #[serde(default = "default_history")]
    pub history_periods: u32,
    #[serde(default)]
    pub method: Option<ForecastMethod>,
}

fn default_horizon() -> u32 {
    3
}
fn default_history() -> u32 {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ForecastPoint {
    pub period_index: i32,
    pub period_label: String,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ForecastResponse {
    pub series: String,
    pub metric: String,
    pub method: ForecastMethod,
    /// Explicit inputs used — DoD: every forecast exposes inputs + method.
    pub inputs: ForecastInputs,
    pub history: Vec<ForecastPoint>,
    pub forecast: Vec<ForecastPoint>,
    pub unit: MetricUnit,
    pub explainability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ForecastInputs {
    pub history_values: Vec<i64>,
    pub history_periods: u32,
    pub horizon_periods: u32,
    pub method_params: serde_json::Value,
}

pub fn map_series_to_metric(series: &str) -> Option<&'static str> {
    match series {
        "revenue" | "revenue_issued" => Some("revenue_issued"),
        "cash_flow" | "cash_collected" => Some("cash_collected"),
        "pipeline" | "pipeline_amount" => Some("pipeline_amount"),
        "headcount" | "headcount_proxy" => Some("headcount_proxy"),
        "expenses" | "expenses_total" => Some("expenses_total"),
        _ => None,
    }
}

/// Pure forecast from history values (no DB) — used by API after loading rollups.
pub fn forecast_from_history(
    series: &str,
    history: &[i64],
    horizon: u32,
    method: ForecastMethod,
) -> Result<ForecastResponse, String> {
    let metric_name = map_series_to_metric(series).ok_or_else(|| {
        format!("unknown forecast series '{series}' (want revenue|cash_flow|pipeline|headcount)")
    })?;
    let metric = get_metric(metric_name).ok_or_else(|| "metric missing".to_string())?;

    let hist_points: Vec<ForecastPoint> = history
        .iter()
        .enumerate()
        .map(|(i, v)| ForecastPoint {
            period_index: i as i32 - history.len() as i32 + 1,
            period_label: format!("history_{}", i + 1),
            value: *v,
        })
        .collect();

    let (forecast_vals, explain, params) = match method {
        ForecastMethod::TrailingAverage => {
            let avg = if history.is_empty() {
                0
            } else {
                history.iter().sum::<i64>() / history.len() as i64
            };
            let vals = vec![avg; horizon as usize];
            (
                vals,
                format!(
                    "Trailing average of {} history periods = {avg}; projected flat for {horizon} periods.",
                    history.len()
                ),
                serde_json::json!({ "average": avg }),
            )
        }
        ForecastMethod::LinearTrend => {
            let n = history.len() as f64;
            if n < 2.0 {
                let last = *history.last().unwrap_or(&0);
                (
                    vec![last; horizon as usize],
                    "Fewer than 2 history points; held last value constant.".into(),
                    serde_json::json!({ "slope": 0, "intercept": last }),
                )
            } else {
                // OLS on x=0..n-1
                let sum_x: f64 = (0..history.len()).map(|i| i as f64).sum();
                let sum_y: f64 = history.iter().map(|v| *v as f64).sum();
                let sum_xx: f64 = (0..history.len()).map(|i| (i as f64).powi(2)).sum();
                let sum_xy: f64 = history
                    .iter()
                    .enumerate()
                    .map(|(i, v)| i as f64 * (*v as f64))
                    .sum();
                let denom = n * sum_xx - sum_x * sum_x;
                let slope = if denom.abs() < f64::EPSILON {
                    0.0
                } else {
                    (n * sum_xy - sum_x * sum_y) / denom
                };
                let intercept = (sum_y - slope * sum_x) / n;
                let vals: Vec<i64> = (0..horizon as usize)
                    .map(|h| {
                        let x = history.len() as f64 + h as f64;
                        (intercept + slope * x).round() as i64
                    })
                    .collect();
                (
                    vals,
                    format!(
                        "Linear trend OLS slope={slope:.4} intercept={intercept:.4} over {} periods.",
                        history.len()
                    ),
                    serde_json::json!({ "slope": slope, "intercept": intercept }),
                )
            }
        }
    };

    let now = Utc::now().date_naive();
    let forecast_points: Vec<ForecastPoint> = forecast_vals
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let d = now + Duration::days(7 * (i as i64 + 1));
            ForecastPoint {
                period_index: (i + 1) as i32,
                period_label: d.to_string(),
                value: *v,
            }
        })
        .collect();

    Ok(ForecastResponse {
        series: series.into(),
        metric: metric.name.clone(),
        method,
        inputs: ForecastInputs {
            history_values: history.to_vec(),
            history_periods: history.len() as u32,
            horizon_periods: horizon,
            method_params: params,
        },
        history: hist_points,
        forecast: forecast_points,
        unit: metric.unit,
        explainability: explain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forecast_exposes_method_and_inputs() {
        let hist = vec![100, 110, 120, 130];
        let res = forecast_from_history("revenue", &hist, 2, ForecastMethod::LinearTrend).unwrap();
        assert_eq!(res.method, ForecastMethod::LinearTrend);
        assert_eq!(res.inputs.history_values, hist);
        assert!(!res.explainability.is_empty());
        assert_eq!(res.forecast.len(), 2);
        assert_eq!(res.metric, "revenue_issued");
    }

    #[test]
    fn trailing_average_flat() {
        let hist = vec![10, 20, 30];
        let res =
            forecast_from_history("cash_flow", &hist, 3, ForecastMethod::TrailingAverage).unwrap();
        assert_eq!(res.forecast[0].value, 20);
        assert_eq!(res.forecast.len(), 3);
    }
}
