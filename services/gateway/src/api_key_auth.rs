//! Organization API-key authentication, exchange, and per-key rate limiting
//! (Phase 3.3).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use companyos_auth_token::{hash_token, AccessClaims};
use companyos_errors::{AppError, ErrorCode, Problem};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::public_routes::{self, scopes_allow};

/// Dual-publish deprecation exercise (rate_limit_rpm → rate_limit_per_minute).
pub const DEPRECATION_SUNSET: &str = "Sat, 27 Feb 2027 00:00:00 GMT";
pub const DEPRECATION_LINK: &str =
    "</docs/developers/deprecation.md>; rel=\"deprecation\"; type=\"text/markdown\"";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRequest {
    pub key_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeResponse {
    pub access_token: String,
    pub api_key_id: String,
    pub org_id: String,
    pub scopes: Vec<String>,
    pub rate_limit_per_minute: i32,
    #[serde(default)]
    pub rate_limit_rpm: Option<i32>,
}

/// Snapshot of rate-limit counters to attach as response headers.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitInfo {
    pub limit: u32,
    pub remaining: u32,
    pub reset_epoch_secs: u64,
}

impl RateLimitInfo {
    pub fn apply_headers(&self, headers: &mut HeaderMap) {
        if let Ok(v) = HeaderValue::from_str(&self.limit.to_string()) {
            headers.insert(HeaderName::from_static("ratelimit-limit"), v);
        }
        if let Ok(v) = HeaderValue::from_str(&self.remaining.to_string()) {
            headers.insert(HeaderName::from_static("ratelimit-remaining"), v);
        }
        if let Ok(v) = HeaderValue::from_str(&self.reset_epoch_secs.to_string()) {
            headers.insert(HeaderName::from_static("ratelimit-reset"), v);
        }
    }
}

/// Successful API-key (or JWT-as-api-key) auth outcome used by the proxy layer.
#[derive(Debug, Clone)]
pub struct ApiKeyAuthContext {
    pub claims: AccessClaims,
    /// Minted access JWT to put in `Authorization` before upstream proxy.
    pub minted_bearer: Option<String>,
    pub rate_limit: RateLimitInfo,
    pub api_key_id: String,
}

/// In-memory fixed-window rate limiter (per api_key_id per UTC minute).
pub struct ApiKeyRateLimiter {
    inner: Mutex<HashMap<String, Window>>,
}

struct Window {
    /// Unix minute bucket (epoch_secs / 60).
    minute: u64,
    count: u32,
}

impl ApiKeyRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn now_epoch() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Check and increment. Returns Ok(info) when allowed, Err(info) when exceeded.
    pub fn check_and_hit(&self, api_key_id: &str, limit: u32) -> Result<RateLimitInfo, RateLimitInfo> {
        let limit = limit.max(1);
        let now = Self::now_epoch();
        let minute = now / 60;
        let reset = (minute + 1) * 60;

        let mut g = self.inner.lock().expect("api key rate limiter");
        // Opportunistic prune of stale windows.
        if g.len() > 10_000 {
            g.retain(|_, w| w.minute >= minute.saturating_sub(1));
        }
        let entry = g.entry(api_key_id.to_string()).or_insert(Window {
            minute,
            count: 0,
        });
        if entry.minute != minute {
            entry.minute = minute;
            entry.count = 0;
        }
        if entry.count >= limit {
            return Err(RateLimitInfo {
                limit,
                remaining: 0,
                reset_epoch_secs: reset,
            });
        }
        entry.count += 1;
        Ok(RateLimitInfo {
            limit,
            remaining: limit.saturating_sub(entry.count),
            reset_epoch_secs: reset,
        })
    }
}

impl Default for ApiKeyRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Optional Redis-backed fixed-window limiter. Falls back to in-memory on error.
pub async fn check_rate_limit(
    memory: &ApiKeyRateLimiter,
    redis_url: Option<&str>,
    api_key_id: &str,
    limit_per_minute: i32,
) -> Result<RateLimitInfo, RateLimitInfo> {
    let limit = limit_per_minute.max(1) as u32;
    if let Some(url) = redis_url {
        match redis_check_and_hit(url, api_key_id, limit).await {
            Ok(outcome) => return outcome,
            Err(e) => {
                warn!(error = %e, "redis rate limit failed; using in-memory");
            }
        }
    }
    memory.check_and_hit(api_key_id, limit)
}

async fn redis_check_and_hit(
    redis_url: &str,
    api_key_id: &str,
    limit: u32,
) -> Result<Result<RateLimitInfo, RateLimitInfo>, String> {
    let client = redis::Client::open(redis_url).map_err(|e| e.to_string())?;
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| e.to_string())?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let minute = now / 60;
    let reset = (minute + 1) * 60;
    let key = format!("companyos:api_key_rl:{api_key_id}:{minute}");

    let count: u32 = redis::cmd("INCR")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(|e| e.to_string())?;
    if count == 1 {
        let _: () = redis::cmd("EXPIRE")
            .arg(&key)
            .arg(90i64)
            .query_async(&mut conn)
            .await
            .map_err(|e| e.to_string())?;
    }

    if count > limit {
        Ok(Err(RateLimitInfo {
            limit,
            remaining: 0,
            reset_epoch_secs: reset,
        }))
    } else {
        Ok(Ok(RateLimitInfo {
            limit,
            remaining: limit.saturating_sub(count),
            reset_epoch_secs: reset,
        }))
    }
}

pub fn rate_limited_response(request_id: &str, info: RateLimitInfo) -> Response {
    let retry_after = info
        .reset_epoch_secs
        .saturating_sub(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
        .max(1);
    let problem = Problem::new(
        ErrorCode::TooManyRequests,
        request_id,
        "API key rate limit exceeded",
    );
    let mut res = (StatusCode::TOO_MANY_REQUESTS, Json(problem)).into_response();
    res.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    if let Ok(v) = HeaderValue::from_str(&retry_after.to_string()) {
        res.headers_mut()
            .insert(axum::http::header::RETRY_AFTER, v);
    }
    info.apply_headers(res.headers_mut());
    apply_deprecation_headers(res.headers_mut());
    res
}

pub fn apply_deprecation_headers(headers: &mut HeaderMap) {
    headers.insert(
        HeaderName::from_static("deprecation"),
        HeaderValue::from_static("true"),
    );
    headers.insert(
        HeaderName::from_static("sunset"),
        HeaderValue::from_static(DEPRECATION_SUNSET),
    );
    if let Ok(v) = HeaderValue::from_str(DEPRECATION_LINK) {
        headers.insert(HeaderName::from_static("link"), v);
    }
}

/// Hash the raw key and exchange it at core for a short-lived access JWT.
pub async fn exchange_api_key(
    client: &reqwest::Client,
    core_url: &str,
    raw_key: &str,
    request_id: &str,
) -> Result<ExchangeResponse, AppError> {
    let key_hash = hash_token(raw_key);
    let url = format!(
        "{}/api/v1/internal/api-keys/exchange",
        core_url.trim_end_matches('/')
    );
    let resp = client
        .post(&url)
        .header("x-request-id", request_id)
        .json(&ExchangeRequest { key_hash })
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::ServiceUnavailable,
                request_id,
                format!("api key exchange unreachable: {e}"),
            )
        })?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let code = if status.as_u16() == 401 {
            ErrorCode::Unauthorized
        } else if status.as_u16() == 403 {
            ErrorCode::Forbidden
        } else {
            ErrorCode::Unauthorized
        };
        let detail = if body.is_empty() {
            format!("api key exchange failed ({status})")
        } else {
            // Prefer problem detail when present.
            serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("detail")
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| format!("api key exchange failed ({status}): {body}"))
        };
        return Err(AppError::new(code, request_id, detail));
    }

    resp.json::<ExchangeResponse>().await.map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("invalid exchange response: {e}"),
        )
    })
}

/// Enforce public allowlist + required scope for an API-key caller.
pub fn enforce_api_key_route(
    method: &str,
    path: &str,
    scopes: &[String],
    request_id: &str,
) -> Result<(), AppError> {
    if public_routes::is_openapi_public(path) {
        return Ok(());
    }
    let Some(required) = public_routes::public_permission_for(method, path) else {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "API keys may only access public API routes",
        ));
    };
    if !scopes_allow(scopes, &required) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!("API key missing required scope `{}`", required.as_str()),
        ));
    }
    Ok(())
}

/// Fire-and-forget usage log (core internal usage endpoint is optional).
pub fn log_api_key_usage(
    client: reqwest::Client,
    core_url: String,
    api_key_id: String,
    org_id: String,
    method: String,
    path: String,
    status_code: u16,
    duration_ms: u128,
    request_id: String,
) {
    tokio::spawn(async move {
        info!(
            %api_key_id,
            %org_id,
            %method,
            %path,
            status_code,
            duration_ms,
            %request_id,
            "api_key_usage"
        );
        let url = format!(
            "{}/api/v1/internal/api-keys/usage",
            core_url.trim_end_matches('/')
        );
        let body = serde_json::json!({
            "api_key_id": api_key_id,
            "org_id": org_id,
            "route": path,
            "method": method,
            "status_code": status_code,
            "duration_ms": duration_ms as i64,
        });
        let _ = client
            .post(&url)
            .header("x-request-id", &request_id)
            .json(&body)
            .timeout(Duration::from_secs(3))
            .send()
            .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_trips() {
        let lim = ApiKeyRateLimiter::new();
        assert!(lim.check_and_hit("k1", 2).is_ok());
        assert!(lim.check_and_hit("k1", 2).is_ok());
        assert!(lim.check_and_hit("k1", 2).is_err());
    }
}
