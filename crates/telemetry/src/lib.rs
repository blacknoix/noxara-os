//! Telemetry helpers: structured JSON tracing and health probes.
//!
//! **Never log secrets** (tokens, passwords, connection strings with credentials).

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;
use serde::Serialize;
use tracing_subscriber::EnvFilter;

/// Install a JSON structured logger suitable for local + container runtimes.
///
/// Filters out common secret-bearing field names via documentation and
/// `redact_value` helper — callers must not pass secrets into span fields.
pub fn init_tracing(service_name: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_span_list(false)
        .with_target(true)
        .with_env_filter(filter);
    // Avoid panicking if called twice in tests.
    let _ = subscriber.try_init();
    tracing::info!(service = service_name, "telemetry initialized");
}

/// Field names that must never appear in logs.
pub const FORBIDDEN_LOG_KEYS: &[&str] = &[
    "password",
    "secret",
    "token",
    "authorization",
    "api_key",
    "database_url",
    "private_key",
];

/// Returns true if a log key looks like a secret carrier.
pub fn is_forbidden_log_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    FORBIDDEN_LOG_KEYS
        .iter()
        .any(|k| lower == *k || lower.contains(k))
}

/// Redact a value when the key is forbidden.
pub fn redact_value(key: &str, value: &str) -> String {
    if is_forbidden_log_key(key) {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}

#[derive(Debug, Serialize)]
pub struct HealthBody {
    pub status: &'static str,
    pub service: String,
}

async fn livez() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

async fn healthz(service: String) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(HealthBody {
            status: "ok",
            service,
        }),
    )
}

async fn readyz_ok() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "ready" })),
    )
}

/// Build `/livez`, `/readyz`, `/healthz` routes.
///
/// `ready` may be customized by the caller; default reports ready.
pub fn health_router(service_name: impl Into<String>) -> Router {
    let name = service_name.into();
    let name_hz = name.clone();
    Router::new()
        .route("/livez", get(livez))
        .route("/readyz", get(readyz_ok))
        .route(
            "/healthz",
            get(move || {
                let n = name_hz.clone();
                async move { healthz(n).await }
            }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbids_secret_keys() {
        assert!(is_forbidden_log_key("password"));
        assert!(is_forbidden_log_key("Authorization"));
        assert!(is_forbidden_log_key("api_key"));
        assert!(is_forbidden_log_key("DATABASE_URL"));
        assert!(!is_forbidden_log_key("org_id"));
        assert!(!is_forbidden_log_key("request_id"));
    }

    #[test]
    fn redacts_secrets() {
        assert_eq!(redact_value("token", "supersecret"), "[REDACTED]");
        assert_eq!(redact_value("org_id", "org_abc"), "org_abc");
    }

    #[tokio::test]
    async fn health_routes_respond() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = health_router("test-svc");
        for path in ["/livez", "/readyz", "/healthz"] {
            let res = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "path {path}");
        }
    }
}
