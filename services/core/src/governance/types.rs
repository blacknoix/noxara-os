//! Governance API types (OpenAPI schemas) — Phase 2.6.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AccessReviewQuery {
    pub permission_id: String,
    /// RFC3339 timestamp.
    pub period_start: String,
    /// RFC3339 timestamp.
    pub period_end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EntitlementRow {
    pub user_id: String,
    pub email: String,
    pub role_key: String,
    pub permission_id: String,
    pub effective_from: String,
    pub effective_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WhoCouldSeeResponse {
    pub items: Vec<EntitlementRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuditReadRow {
    pub user_id: String,
    pub email: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub created_at: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WhoDidSeeResponse {
    pub items: Vec<AuditReadRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AccessReviewKickoffRequest {
    pub permission_id: String,
    /// RFC3339 timestamp.
    pub period_start: String,
    /// RFC3339 timestamp.
    pub period_end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AccessReviewRunView {
    pub id: String,
    pub status: String,
    pub permission_id: String,
    pub period_start: String,
    pub period_end: String,
    pub summary: serde_json::Value,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuditVerifyRequest {
    /// `YYYY-MM`, or `None` to verify all partitions for the org.
    pub partition_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuditVerifyResponse {
    pub ok: bool,
    pub partitions_checked: i64,
    pub rows_checked: i64,
    pub first_break: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RetentionConfigView {
    pub default_retention_days: i32,
    pub overrides: serde_json::Value,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateRetentionRequest {
    pub default_retention_days: Option<i32>,
    pub overrides: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RetentionDryRunResponse {
    pub cutoff_date: String,
    pub partitions: Vec<String>,
    pub would_affect_estimate: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyView {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyListResponse {
    pub items: Vec<ApiKeyView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub scopes: Vec<String>,
    /// RFC3339 timestamp, or `None` for no expiry.
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateApiKeyResponse {
    pub key: ApiKeyView,
    /// Raw secret — returned only once, at creation time.
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RotateApiKeyResponse {
    pub key: ApiKeyView,
    /// Raw secret — returned only once, at rotation time.
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookEndpointView {
    pub id: String,
    pub url: String,
    pub description: String,
    pub event_types: Vec<String>,
    pub secret_prefix: String,
    pub status: String,
    pub failure_count: i32,
    pub last_delivery_at: Option<String>,
    pub created_at: String,
    pub disabled_at: Option<String>,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookEndpointListResponse {
    pub items: Vec<WebhookEndpointView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateWebhookEndpointRequest {
    pub url: String,
    #[serde(default)]
    pub description: String,
    pub event_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateWebhookEndpointResponse {
    pub endpoint: WebhookEndpointView,
    /// Signing secret — returned only once, at creation time.
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RotateWebhookSecretResponse {
    pub endpoint: WebhookEndpointView,
    /// Signing secret — returned only once, at rotation time.
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DisableWebhookRequest {
    #[serde(default = "default_disable_reason")]
    pub reason: String,
}

fn default_disable_reason() -> String {
    "disabled_by_operator".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookDeliveryView {
    pub id: String,
    pub endpoint_id: String,
    pub event_subject: String,
    pub event_type: String,
    pub attempt: i32,
    pub status: String,
    pub status_code: Option<i32>,
    pub response_body: Option<String>,
    pub delivered_at: Option<String>,
    pub next_retry_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookDeliveryListResponse {
    pub items: Vec<WebhookDeliveryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReplayWebhookResponse {
    pub delivery: WebhookDeliveryView,
}

/// Internal gateway ↔ core API key exchange (not part of the public catalogue).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyExchangeRequest {
    /// SHA-256 hex hash of the raw API key (gateway hashes before calling).
    pub key_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyExchangeResponse {
    /// Short-lived access JWT with `api_key_id` + effective `scopes`.
    pub access_token: String,
    pub api_key_id: String,
    pub org_id: String,
    pub scopes: Vec<String>,
    pub rate_limit_per_minute: i32,
    /// Deprecated dual-published field (Phase 3.3 deprecation exercise).
    /// Prefer `rate_limit_per_minute`. Present for 180 days.
    #[serde(default)]
    pub rate_limit_rpm: Option<i32>,
}
