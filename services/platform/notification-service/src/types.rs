//! DTOs for the notification API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NotificationItemDto {
    pub id: String,
    pub title: String,
    pub body: String,
    pub href: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FeedResponse {
    pub items: Vec<NotificationItemDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PreferenceDto {
    pub channel: String,
    pub enabled: bool,
    pub quiet_hours_start: Option<String>,
    pub quiet_hours_end: Option<String>,
    pub digest_cron: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PreferencesResponse {
    pub preferences: Vec<PreferenceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PutPreferencesRequest {
    pub preferences: Vec<PreferenceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SseTokenResponse {
    /// Opaque token for gateway SSE proxy (Phase 1.8 stub).
    pub token: String,
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IngestResponse {
    pub accepted: bool,
    pub duplicate: bool,
    pub notified: u32,
    pub skipped: u32,
}
