//! Marketplace calendar connector path — mock in CI, never live Microsoft Graph.

use companyos_errors::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gateway_client::forward_user_request;
use crate::state::AppState;
use axum::http::{HeaderMap, Method, StatusCode};
use uuid::Uuid;

pub const CALENDAR_MICROSOFT: &str = "calendar.microsoft";
pub const MOCK_CONNECTOR_HEADER: &str = "x-mock-calendar-connector";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarMeetingMaterial {
    pub calendar_event_id: String,
    pub connector_key: String,
    pub title: String,
    pub transcript: String,
    pub summary_markdown: String,
    pub action_items: Vec<Value>,
}

/// True when we must not call live Microsoft Graph.
pub fn use_mock_calendar() -> bool {
    match std::env::var("AI_CALENDAR_PROVIDER") {
        Ok(v) if v.eq_ignore_ascii_case("mock") || v.eq_ignore_ascii_case("fixture") => true,
        Ok(v) if v.eq_ignore_ascii_case("microsoft") || v.eq_ignore_ascii_case("live") => false,
        _ => {
            // No live creds in CI/local by default.
            std::env::var("MICROSOFT_GRAPH_CLIENT_ID").is_err()
                && std::env::var("MS_GRAPH_TOKEN").is_err()
        }
    }
}

/// Resolve connector identity: mock header (tests) or marketplace install check.
pub async fn resolve_calendar_connector(
    state: &AppState,
    bearer: &str,
    headers: &HeaderMap,
    user_id: Uuid,
    request_id: &str,
) -> Result<String, AppError> {
    if let Some(mock) = headers
        .get(MOCK_CONNECTOR_HEADER)
        .and_then(|v| v.to_str().ok())
    {
        let key = mock.trim();
        if key.is_empty() {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                "empty X-Mock-Calendar-Connector",
            ));
        }
        return Ok(key.to_string());
    }

    // Prefer marketplace installs via gateway; if unreachable, allow mock provider only.
    let (status, body) = forward_user_request(
        state,
        bearer,
        Method::GET,
        "/api/v1/marketplace/installs",
        None,
        false,
        user_id,
        request_id,
    )
    .await
    .unwrap_or((StatusCode::SERVICE_UNAVAILABLE, Value::Null));

    if status.is_success() {
        let items = body
            .get("items")
            .or_else(|| body.get("installs"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let installed = items.iter().any(|item| {
            let key = item
                .get("connector_key")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("active");
            key == CALENDAR_MICROSOFT && (status == "active" || status.is_empty())
        });
        if installed {
            return Ok(CALENDAR_MICROSOFT.to_string());
        }
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "calendar.microsoft connector is not installed",
        ));
    }

    if use_mock_calendar() {
        // CI without marketplace service: prove path via connector identity default.
        return Ok(CALENDAR_MICROSOFT.to_string());
    }

    Err(AppError::new(
        ErrorCode::ServiceUnavailable,
        request_id,
        "unable to verify marketplace calendar connector install",
    ))
}

pub fn mock_meeting_material(
    calendar_event_id: &str,
    title: Option<&str>,
    transcript: Option<&str>,
) -> CalendarMeetingMaterial {
    let title = title
        .map(str::to_string)
        .unwrap_or_else(|| format!("Mock meeting {calendar_event_id}"));
    let transcript = transcript.map(str::to_string).unwrap_or_else(|| {
        format!(
            "Transcript for {title}.\n\
             Alice: Pipeline review — Acme deal is stale after 18 days.\n\
             Bob: Invoice INV-1001 for Acme is overdue by 12 days.\n\
             Alice: Let's schedule follow-up and propose a payment reminder.\n\
             Bob: Also note Northwind renewal next month."
        )
    });
    let summary_markdown = format!(
        "## {title}\n\n\
         - Reviewed stale Acme deal and overdue Acme invoice.\n\
         - Agreed to follow up on deal activity and send a payment reminder (proposal only).\n\
         - Noted upcoming Northwind renewal.\n"
    );
    let action_items = vec![
        serde_json::json!({
            "title": "Follow up on Acme deal",
            "owner": "Alice",
            "proposal_tool": "create_deal_note",
        }),
        serde_json::json!({
            "title": "Draft payment reminder for overdue invoice",
            "owner": "Bob",
            "proposal_tool": "draft_follow_up_activity",
        }),
    ];
    CalendarMeetingMaterial {
        calendar_event_id: calendar_event_id.to_string(),
        connector_key: CALENDAR_MICROSOFT.to_string(),
        title,
        transcript,
        summary_markdown,
        action_items,
    }
}

/// Fetch meeting material via connector path. Never calls live Microsoft Graph in mock mode.
pub async fn fetch_meeting_material(
    connector_key: &str,
    calendar_event_id: &str,
    title: Option<&str>,
    transcript: Option<&str>,
    request_id: &str,
) -> Result<CalendarMeetingMaterial, AppError> {
    if !use_mock_calendar() && connector_key == CALENDAR_MICROSOFT {
        return Err(AppError::new(
            ErrorCode::ServiceUnavailable,
            request_id,
            "live Microsoft Graph calendar is not enabled in this environment",
        ));
    }
    let mut material = mock_meeting_material(calendar_event_id, title, transcript);
    material.connector_key = connector_key.to_string();
    Ok(material)
}
