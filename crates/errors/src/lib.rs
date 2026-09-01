//! RFC 9457 Problem Details for HTTP APIs (`application/problem+json`).
//!
//! Stable machine-readable `code` values plus a `request_id` for correlation.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

/// Stable problem codes used across services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    Forbidden,
    NotFound,
    ValidationFailed,
    Conflict,
    TenancyViolation,
    Internal,
    ServiceUnavailable,
    /// HTTP 429 — auth rate limits / progressive delays.
    TooManyRequests,
    /// Account temporarily locked after brute-force.
    AccountLocked,
    /// MFA challenge required before access token issuance.
    MfaRequired,
    /// Plan/feature flag disabled (e.g. SSO).
    FeatureDisabled,
    /// Data residency / cross-region access denied (HTTP 451).
    ResidencyViolation,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::ValidationFailed => "validation_failed",
            Self::Conflict => "conflict",
            Self::TenancyViolation => "tenancy_violation",
            Self::Internal => "internal",
            Self::ServiceUnavailable => "service_unavailable",
            Self::TooManyRequests => "too_many_requests",
            Self::AccountLocked => "account_locked",
            Self::MfaRequired => "mfa_required",
            Self::FeatureDisabled => "feature_disabled",
            Self::ResidencyViolation => "residency_violation",
        }
    }

    pub fn status(self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::ValidationFailed => StatusCode::BAD_REQUEST,
            Self::Conflict => StatusCode::CONFLICT,
            Self::TenancyViolation => StatusCode::FORBIDDEN,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::AccountLocked => StatusCode::FORBIDDEN,
            Self::MfaRequired => StatusCode::UNAUTHORIZED,
            Self::FeatureDisabled => StatusCode::FORBIDDEN,
            // RFC 7725 — Unavailable For Legal Reasons (data residency).
            Self::ResidencyViolation => StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
        }
    }
}

/// RFC 9457 problem+json body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Problem {
    /// URI reference identifying the problem type (stable).
    #[serde(rename = "type")]
    pub type_uri: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Stable CompanyOS error code.
    pub code: String,
    /// Correlation id for this request.
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

impl Problem {
    pub fn new(code: ErrorCode, request_id: impl Into<String>, detail: impl Into<String>) -> Self {
        let status = code.status();
        Self {
            type_uri: format!("https://companyos.dev/problems/{}", code.as_str()),
            title: status.canonical_reason().unwrap_or("Error").to_string(),
            status: status.as_u16(),
            detail: Some(detail.into()),
            code: code.as_str().to_string(),
            request_id: request_id.into(),
            instance: None,
        }
    }
}

/// Application error that maps to a Problem response.
#[derive(Debug, Clone)]
pub struct AppError {
    pub code: ErrorCode,
    pub detail: String,
    pub request_id: String,
}

impl AppError {
    pub fn new(code: ErrorCode, request_id: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            request_id: request_id.into(),
        }
    }

    pub fn into_problem(self) -> Problem {
        Problem::new(self.code, self.request_id, self.detail)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.code.status();
        let problem = self.into_problem();
        let mut res = (status, Json(problem)).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/problem+json"),
        );
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_has_stable_code_and_request_id() {
        let p = Problem::new(ErrorCode::NotFound, "req_abc", "hello missing");
        assert_eq!(p.code, "not_found");
        assert_eq!(p.request_id, "req_abc");
        assert_eq!(p.status, 404);
        assert!(p.type_uri.contains("not_found"));
    }

    #[test]
    fn serde_shape_is_rfc9457() {
        let p = Problem::new(ErrorCode::Forbidden, "r1", "denied");
        let v: serde_json::Value = serde_json::to_value(&p).unwrap();
        assert!(v.get("type").is_some());
        assert!(v.get("title").is_some());
        assert!(v.get("status").is_some());
        assert_eq!(v["code"], "forbidden");
        assert_eq!(v["request_id"], "r1");
    }

    #[test]
    fn all_codes_have_unique_strings() {
        let codes = [
            ErrorCode::Unauthorized,
            ErrorCode::Forbidden,
            ErrorCode::NotFound,
            ErrorCode::ValidationFailed,
            ErrorCode::Conflict,
            ErrorCode::TenancyViolation,
            ErrorCode::Internal,
            ErrorCode::ServiceUnavailable,
            ErrorCode::TooManyRequests,
            ErrorCode::AccountLocked,
            ErrorCode::MfaRequired,
            ErrorCode::FeatureDisabled,
            ErrorCode::ResidencyViolation,
        ];
        let mut set = std::collections::HashSet::new();
        for c in codes {
            assert!(set.insert(c.as_str()));
        }
    }

    #[test]
    fn residency_violation_is_http_451() {
        assert_eq!(
            ErrorCode::ResidencyViolation.status(),
            StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
        );
        assert_eq!(
            ErrorCode::ResidencyViolation.as_str(),
            "residency_violation"
        );
    }
}
