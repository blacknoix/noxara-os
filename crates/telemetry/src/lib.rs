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

/// In-process RED (Rate / Errors / Duration) counters for hot paths.
///
/// This is a lightweight OpenTelemetry-*shaped* meter that does not require
/// an OTLP exporter in CI. Services record spans on list/create hot paths;
/// `/metrics` (when mounted) exposes snapshots for dashboards and before/after
/// proofs. Keys must include `org_id` when tenant-scoped.
#[derive(Debug, Default)]
pub struct RedMeter {
    inner: std::sync::Mutex<std::collections::HashMap<String, RedCounters>>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct RedCounters {
    pub requests: u64,
    pub errors: u64,
    pub duration_ms_total: u64,
    pub duration_ms_max: u64,
    pub duration_ms_last: u64,
}

impl RedMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one request. `key` should be `{org_id}:{route}` or `{route}` for
    /// process-wide aggregates. Never put secrets in `key`.
    pub fn record(&self, key: impl Into<String>, duration_ms: u64, is_error: bool) {
        let key = key.into();
        if is_forbidden_log_key(&key) {
            tracing::warn!("refusing to record RED metric with forbidden key shape");
            return;
        }
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.entry(key).or_default();
        entry.requests = entry.requests.saturating_add(1);
        if is_error {
            entry.errors = entry.errors.saturating_add(1);
        }
        entry.duration_ms_total = entry.duration_ms_total.saturating_add(duration_ms);
        entry.duration_ms_last = duration_ms;
        if duration_ms > entry.duration_ms_max {
            entry.duration_ms_max = duration_ms;
        }
    }

    pub fn snapshot(&self) -> std::collections::BTreeMap<String, RedCounters> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn get(&self, key: &str) -> Option<RedCounters> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.get(key).cloned()
    }
}

/// Global process meter for services that do not thread state (tests + libs).
pub fn global_red_meter() -> &'static RedMeter {
    static METER: std::sync::OnceLock<RedMeter> = std::sync::OnceLock::new();
    METER.get_or_init(RedMeter::new)
}

/// Timing helper — call [`RedTimer::finish`] (or drop) to record.
pub struct RedTimer {
    meter: &'static RedMeter,
    key: String,
    start: std::time::Instant,
    error: bool,
}

impl RedTimer {
    pub fn start(key: impl Into<String>) -> Self {
        Self {
            meter: global_red_meter(),
            key: key.into(),
            start: std::time::Instant::now(),
            error: false,
        }
    }

    pub fn mark_error(&mut self) {
        self.error = true;
    }

    pub fn finish(mut self) -> u64 {
        let ms = self.start.elapsed().as_millis() as u64;
        self.meter.record(&self.key, ms, self.error);
        // Prevent Drop from double-recording.
        self.key.clear();
        ms
    }
}

impl Drop for RedTimer {
    fn drop(&mut self) {
        if self.key.is_empty() {
            return;
        }
        let ms = self.start.elapsed().as_millis() as u64;
        self.meter.record(&self.key, ms, self.error);
    }
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

    #[test]
    fn red_meter_records_rate_errors_duration() {
        let meter = RedMeter::new();
        meter.record("org_a:list_invoices", 12, false);
        meter.record("org_a:list_invoices", 40, true);
        let snap = meter.get("org_a:list_invoices").unwrap();
        assert_eq!(snap.requests, 2);
        assert_eq!(snap.errors, 1);
        assert_eq!(snap.duration_ms_total, 52);
        assert_eq!(snap.duration_ms_max, 40);
    }

    #[test]
    fn red_timer_finishes_once() {
        let key = format!("test_timer_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let timer = RedTimer::start(key.clone());
        let _ms = timer.finish();
        let snap = global_red_meter().get(&key).unwrap();
        assert_eq!(snap.requests, 1);
    }
}
