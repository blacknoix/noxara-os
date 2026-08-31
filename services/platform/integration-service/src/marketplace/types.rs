//! Wire types and status constants for the marketplace.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const KIND_FIRST_PARTY: &str = "first_party";
pub const KIND_THIRD_PARTY: &str = "third_party";

pub const LISTING_DRAFT: &str = "draft";
pub const LISTING_SUBMITTED: &str = "submitted";
pub const LISTING_IN_REVIEW: &str = "in_review";
pub const LISTING_APPROVED: &str = "approved";
pub const LISTING_REJECTED: &str = "rejected";
pub const LISTING_PUBLISHED: &str = "published";
pub const LISTING_SUSPENDED: &str = "suspended";

pub const INSTALL_ACTIVE: &str = "active";
pub const INSTALL_REVOKED: &str = "revoked";

pub const TOKEN_ACCESS: &str = "access";
pub const TOKEN_REFRESH: &str = "refresh";

/// Access tokens are short-lived; refresh rotates them.
pub const ACCESS_TOKEN_TTL_SECS: i64 = 3600;
pub const REFRESH_TOKEN_TTL_SECS: i64 = 60 * 60 * 24 * 30;
/// Authorization codes are single-use and expire quickly.
pub const AUTH_CODE_TTL_SECS: i64 = 300;

/// One item of the publisher review checklist.
///
/// Items whose `id` starts with `security_` gate `security_review_completed`,
/// which in turn gates publication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChecklistItem {
    pub id: String,
    pub label: String,
    pub required: bool,
    #[serde(default)]
    pub completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

impl ChecklistItem {
    pub fn is_security(&self) -> bool {
        self.id.starts_with("security_")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingDto {
    pub id: String,
    pub org_id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub listing_kind: String,
    pub connector_key: Option<String>,
    pub requested_scopes: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub webhook_subscriptions: Vec<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingListResponse {
    pub items: Vec<ListingDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateListingRequest {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub listing_kind: Option<String>,
    #[serde(default)]
    pub connector_key: Option<String>,
    #[serde(default)]
    pub requested_scopes: Vec<String>,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub webhook_subscriptions: Vec<String>,
}

/// `client_secret` is returned exactly once, at creation.
#[derive(Debug, Clone, Serialize)]
pub struct CreateListingResponse {
    pub listing: ListingDto,
    pub oauth_client_id: String,
    pub oauth_client_public_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDto {
    pub id: String,
    pub listing_id: String,
    pub listing_status: String,
    pub checklist: Vec<ChecklistItem>,
    pub security_review_completed: bool,
    pub status: String,
    pub reviewer_notes: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewQueueResponse {
    pub items: Vec<ReviewDto>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChecklistUpdateRequest {
    /// Checklist item ids to mark complete.
    #[serde(default)]
    pub completed_item_ids: Vec<String>,
    #[serde(default)]
    pub reviewer_notes: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RejectRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallDto {
    pub id: String,
    pub org_id: String,
    pub listing_id: String,
    pub listing_slug: String,
    pub listing_name: String,
    pub listing_kind: String,
    pub connector_key: Option<String>,
    pub consented_scopes: Vec<String>,
    pub status: String,
    pub outbound_enabled: bool,
    pub inbound_enabled: bool,
    pub last_error: Option<String>,
    pub installed_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallListResponse {
    pub items: Vec<InstallDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateInstallRequest {
    pub listing_id: String,
    #[serde(default)]
    pub consented_scopes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReconsentRequest {
    pub consented_scopes: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConnectRequest {
    /// Defaults to the listing's requested scopes intersected with the
    /// installer's own permissions.
    #[serde(default)]
    pub consented_scopes: Option<Vec<String>>,
}

/// Plaintext tokens — returned once, never stored or logged.
#[derive(Debug, Clone, Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallCreatedResponse {
    pub install: InstallDto,
    #[serde(flatten)]
    pub tokens: TokenPair,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationDto {
    pub connector_key: String,
    pub install_id: String,
    pub name: String,
    pub status: String,
    pub scopes: Vec<String>,
    pub outbound_enabled: bool,
    pub inbound_enabled: bool,
    pub last_error: Option<String>,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationListResponse {
    pub items: Vec<IntegrationDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizeRequest {
    pub listing_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub consented_scopes: Vec<String>,
    #[serde(default)]
    pub code_challenge: String,
    #[serde(default)]
    pub code_challenge_method: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizeResponse {
    pub code: String,
    pub redirect_uri: String,
    pub expires_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OauthTokenRequest {
    pub grant_type: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub code_verifier: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OauthTokenResponse {
    pub install_id: String,
    #[serde(flatten)]
    pub tokens: TokenPair,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizePermissionRequest {
    pub access_token: String,
    pub permission: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizePermissionResponse {
    pub allowed: bool,
    pub install_id: String,
    pub org_id: String,
    pub permission: String,
    pub scopes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_items_are_detected_by_id_prefix() {
        let item = ChecklistItem {
            id: "security_scope_review".into(),
            label: "l".into(),
            required: true,
            completed: false,
            completed_by: None,
            completed_at: None,
        };
        assert!(item.is_security());
        let other = ChecklistItem {
            id: "listing_metadata".into(),
            ..item.clone()
        };
        assert!(!other.is_security());
    }
}
