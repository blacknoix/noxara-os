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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterDeviceRequest {
    /// ios | android | web | desktop | fake | test
    pub platform: String,
    pub push_token: String,
    pub device_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterDeviceResponse {
    pub id: String,
    pub platform: String,
    pub registered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeviceDto {
    pub id: String,
    pub platform: String,
    pub push_token: String,
    pub device_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeviceListResponse {
    pub items: Vec<DeviceDto>,
}
