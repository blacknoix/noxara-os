//! Gateway network allowlist gate (Phase 4.2).
//!
//! When an org enables `allowlist_enabled`, requests from non-allowlisted
//! source IPs (and without an allowlisted mTLS client id) are rejected.
//! The authoritative policy lives in core (`org_network_policy_lookup`);
//! this module calls the internal network-gate endpoint.

use axum::http::HeaderMap;
use companyos_errors::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct GateRequest {
    org_id: String,
    source_ip: String,
    mtls_client_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GateResponse {
    allowed: bool,
    reason: String,
    #[allow(dead_code)]
    infra_tier: String,
}

/// Extract client IP from common proxy headers, falling back to `fallback`.
pub fn client_ip(headers: &HeaderMap, fallback: &str) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn mtls_client_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-client-cert-cn")
        .or_else(|| headers.get("x-amzn-mtls-clientcert-subject"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Ask core whether this source may reach the org data plane.
pub async fn gate_network(
    client: &reqwest::Client,
    core_url: &str,
    org_public_id: &str,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<(), AppError> {
    let source_ip = client_ip(headers, "127.0.0.1");
    let body = GateRequest {
        org_id: org_public_id.to_string(),
        source_ip,
        mtls_client_id: mtls_client_id(headers),
    };
    let url = format!(
        "{}/api/v1/internal/network-gate",
        core_url.trim_end_matches('/')
    );
    let resp = client
        .post(&url)
        .header("x-request-id", request_id)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::ServiceUnavailable,
                request_id,
                format!("network gate unreachable: {e}"),
            )
        })?;
    if !resp.status().is_success() {
        // Fail closed if the gate endpoint errors while we cannot parse policy.
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "network gate denied (upstream error)",
        ));
    }
    let gate: GateResponse = resp.json().await.map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("network gate decode: {e}"),
        )
    })?;
    if !gate.allowed {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!("network allowlist: {}", gate.reason),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn parses_xff() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("10.1.2.3, 10.0.0.1"),
        );
        assert_eq!(client_ip(&h, "127.0.0.1"), "10.1.2.3");
    }
}
