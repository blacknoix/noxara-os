//! CompanyOS integration service (library).
//!
//! Phase 3.3 — outbound webhook delivery (crypto, dispatcher, SSRF).
//! Phase 3.4 — marketplace listings, review, installs, app tokens, OAuth.

pub mod crypto;
pub mod dispatcher;
pub mod enqueue;
pub mod marketplace;
pub mod sign;
pub mod ssrf;

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_auth_token::KeyRing;
use companyos_events::EventEnvelope;
use serde::Serialize;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use crate::crypto::WebhookDecryptor;
use crate::dispatcher::{dispatch_once, DispatchOptions, DispatchStats};
use crate::enqueue::{enqueue_event, EnqueueResult};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub decryptor: Arc<WebhookDecryptor>,
    /// SSRF policy captured at boot / test setup (not re-read from env per request).
    pub dispatch_opts: DispatchOptions,
    /// Access-token verification keys for session-authenticated marketplace routes.
    pub keyring: KeyRing,
    /// Marketplace URL validation escape hatch for loopback fixtures in tests.
    pub allow_private_urls: bool,
}

impl AppState {
    pub fn new(pool: sqlx::PgPool, decryptor: WebhookDecryptor) -> Self {
        Self::with_dispatch_opts(pool, decryptor, DispatchOptions::from_env())
    }

    pub fn with_dispatch_opts(
        pool: sqlx::PgPool,
        decryptor: WebhookDecryptor,
        dispatch_opts: DispatchOptions,
    ) -> Self {
        Self {
            pool,
            decryptor: Arc::new(decryptor),
            dispatch_opts,
            keyring: marketplace::auth::build_keyring(),
            allow_private_urls: dispatch_opts.allow_private,
        }
    }

    pub fn with_keyring(mut self, keyring: KeyRing) -> Self {
        self.keyring = keyring;
        self
    }

    pub fn with_allow_private_urls(mut self, allow: bool) -> Self {
        self.allow_private_urls = allow;
        self
    }
}

pub fn split_sql(sql: &str) -> Vec<String> {
    let cleaned: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    cleaned
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Webhook tables live in core (`006_webhooks.sql`); marketplace schema is local.
pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    companyos_tenancy::with_schema_migration_lock(pool, || async {
        for migration in [include_str!("../migrations/001_marketplace.sql")] {
            for stmt in split_sql(migration) {
                companyos_tenancy::execute_migration_stmt(pool, &stmt).await?;
            }
        }
        Ok(())
    })
    .await
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
    dispatch_once(&state.pool, &state.decryptor, 25, state.dispatch_opts)
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
        .merge(marketplace::handlers::router())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}
