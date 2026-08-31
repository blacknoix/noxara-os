//! CompanyOS outbound webhook integration service (library) — Phase 3.3.

pub mod crypto;
pub mod dispatcher;
pub mod enqueue;
pub mod sign;
pub mod ssrf;

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_events::EventEnvelope;
use serde::Serialize;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use crate::crypto::WebhookDecryptor;
use crate::dispatcher::{dispatch_once, DispatchStats};
use crate::enqueue::{enqueue_event, EnqueueResult};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub decryptor: Arc<WebhookDecryptor>,
}

impl AppState {
    pub fn new(pool: sqlx::PgPool, decryptor: WebhookDecryptor) -> Self {
        Self {
            pool,
            decryptor: Arc::new(decryptor),
        }
    }
}

/// Tables live in core (`006_webhooks.sql`); no local schema to apply.
pub async fn migrate(_pool: &sqlx::PgPool) -> anyhow::Result<()> {
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct EnqueueResponse {
    pub matched_endpoints: usize,
    pub inserted: usize,
}

#[derive(Debug, Serialize)]
pub struct DispatchOnceResponse {
    pub claimed: usize,
    pub delivered: usize,
    pub failed: usize,
    pub skipped_ssrf: usize,
}

impl From<EnqueueResult> for EnqueueResponse {
    fn from(r: EnqueueResult) -> Self {
        Self {
            matched_endpoints: r.matched_endpoints,
            inserted: r.inserted,
        }
    }
}

impl From<DispatchStats> for DispatchOnceResponse {
    fn from(s: DispatchStats) -> Self {
        Self {
            claimed: s.claimed,
            delivered: s.delivered,
            failed: s.failed,
            skipped_ssrf: s.skipped_ssrf,
        }
    }
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "companyos-integration"
    }))
}

/// Internal: enqueue deliveries for an [`EventEnvelope`] (tests / local wiring).
async fn internal_enqueue(
    State(state): State<AppState>,
    Json(envelope): Json<EventEnvelope>,
) -> Result<Json<EnqueueResponse>, (axum::http::StatusCode, String)> {
    enqueue_event(&state.pool, &envelope)
        .await
        .map(|r| Json(EnqueueResponse::from(r)))
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Internal: process one pending delivery batch.
async fn internal_dispatch_once(
    State(state): State<AppState>,
) -> Result<Json<DispatchOnceResponse>, (axum::http::StatusCode, String)> {
    dispatch_once(&state.pool, &state.decryptor, 25)
        .await
        .map(|s| Json(DispatchOnceResponse::from(s)))
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub fn build_router(state: AppState) -> Router {
    let x_request_id = http::HeaderName::from_static("x-request-id");
    Router::new()
        .route("/healthz", get(healthz))
        .route("/livez", get(healthz))
        .route("/readyz", get(healthz))
        .route("/api/v1/internal/webhooks/enqueue", post(internal_enqueue))
        .route(
            "/api/v1/internal/webhooks/dispatch-once",
            post(internal_dispatch_once),
        )
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}
