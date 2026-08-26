//! Optional HTTP surface for health + lag metrics.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use companyos_outbox::relay::RelayMetrics;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Clone)]
pub struct HttpState {
    pub pool: PgPool,
    pub metrics: Arc<RelayMetrics>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MetricsResponse {
    pub published: u64,
    pub dlq: u64,
    pub lag: u64,
    pub batches: u64,
    pub oldest_unpublished_at: Option<String>,
}

#[utoipa::path(
    get,
    path = "/metrics",
    responses((status = 200, description = "Relay lag snapshot", body = MetricsResponse)),
    tag = "outbox-relay"
)]
pub async fn metrics(State(state): State<HttpState>) -> Json<MetricsResponse> {
    let snap = state.metrics.snapshot();
    let oldest = companyos_outbox::relay::oldest_unpublished_at(&state.pool)
        .await
        .ok()
        .flatten()
        .map(|t| t.to_rfc3339());
    // Refresh lag from DB so /metrics is current even between ticks.
    if let Ok(lag) = companyos_outbox::relay::unpublished_lag(&state.pool).await {
        state
            .metrics
            .lag
            .store(lag, std::sync::atomic::Ordering::Relaxed);
    }
    Json(MetricsResponse {
        published: snap.published,
        dlq: snap.dlq,
        lag: state.metrics.lag.load(std::sync::atomic::Ordering::Relaxed),
        batches: snap.batches,
        oldest_unpublished_at: oldest,
    })
}

pub fn router() -> Router<HttpState> {
    Router::new().route("/metrics", get(metrics))
}
