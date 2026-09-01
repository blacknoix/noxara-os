//! OpenAPI for notification service.

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{devices, feed, ingest, preferences, sse};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        feed::feed,
        feed::mark_read,
        preferences::get_preferences,
        preferences::put_preferences,
        devices::list_devices,
        devices::register_device,
        devices::unregister_device,
        ingest::ingest,
        sse::sse_token,
    ),
    components(schemas(
        NotificationItemDto,
        FeedResponse,
        PreferenceDto,
        PreferencesResponse,
        PutPreferencesRequest,
        MessageResponse,
        SseTokenResponse,
        IngestResponse,
        RegisterDeviceRequest,
        RegisterDeviceResponse,
        DeviceDto,
        DeviceListResponse,
    )),
    tags(
        (name = "notifications", description = "In-app notification feed, preferences, and push device tokens"),
        (name = "notifications-internal", description = "Event ingest (service-to-service)"),
    ),
    info(
        title = "CompanyOS Notification API",
        version = "0.1.0",
        description = "Phase 1.8 + 1.11 — notification fan-out, preferences, push device registration (no live FCM/APNs)."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/notifications/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
