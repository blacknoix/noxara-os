//! Meeting summarisation via marketplace calendar connector path (propose-only).

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde_json::json;
use uuid::Uuid;

use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::calendar::{
    fetch_meeting_material, resolve_calendar_connector, CALENDAR_MICROSOFT,
};
use crate::handlers::common::{enforce_perm, extract_bearer, resolve_principal};
use crate::state::AppState;
use crate::types::{
    CreateMeetingSummaryRequest, MeetingSummariesListResponse, MeetingSummaryView, MessageResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/ai/meeting-summaries",
            get(list_meeting_summaries),
        )
        .route(
            "/api/v1/ai/meeting-summaries/from-calendar",
            post(create_from_calendar),
        )
        .route(
            "/api/v1/ai/meeting-summaries/{id}",
            get(get_meeting_summary),
        )
        .route(
            "/api/v1/ai/meeting-summaries/{id}/accept",
            post(accept_meeting_summary),
        )
        .route(
            "/api/v1/ai/meeting-summaries/{id}/reject",
            post(reject_meeting_summary),
        )
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/meeting-summaries",
    responses((status = 200, body = MeetingSummariesListResponse)),
    tag = "ai"
)]
pub async fn list_meeting_summaries(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<MeetingSummariesListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_meeting_summary_read(), &request_id)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        Option<String>,
        String,
        serde_json::Value,
        String,
        Option<chrono::DateTime<Utc>>,
        Option<Uuid>,
        chrono::DateTime<Utc>,
    )> = sqlx::query_as(
        r#"
        SELECT id, public_id, calendar_event_id, calendar_connector, transcript,
               summary_markdown, action_items, status, accepted_at, accepted_by, created_at
        FROM ai_meeting_summary
        WHERE org_id = $1
        ORDER BY created_at DESC
        LIMIT 50
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let items = rows
        .into_iter()
        .map(|r| row_to_view(r))
        .collect();

    Ok(Json(MeetingSummariesListResponse { items }))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/meeting-summaries/{id}",
    responses((status = 200, body = MeetingSummaryView)),
    tag = "ai"
)]
pub async fn get_meeting_summary(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<MeetingSummaryView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_meeting_summary_read(), &request_id)?;

    let view = load_summary(&state, org_id.as_uuid(), &id, &request_id).await?;
    Ok(Json(view))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/meeting-summaries/from-calendar",
    request_body = CreateMeetingSummaryRequest,
    responses((status = 200, body = MeetingSummaryView)),
    tag = "ai"
)]
pub async fn create_from_calendar(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(req): Json<CreateMeetingSummaryRequest>,
) -> Result<Json<MeetingSummaryView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;
    let bearer = extract_bearer(&headers).unwrap_or_default();

    let principal = resolve_principal(&state, &auth).await?;
    // Creating a suggestion requires read; accept is a separate human commit.
    enforce_perm(&principal, perms::ai_meeting_summary_read(), &request_id)?;

    if req.calendar_event_id.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "calendar_event_id is required",
        ));
    }

    let connector =
        resolve_calendar_connector(&state, &bearer, &headers, user_id, &request_id).await?;
    if connector != CALENDAR_MICROSOFT
        && !headers.contains_key(crate::calendar::MOCK_CONNECTOR_HEADER)
    {
        // Still allow other connector keys when explicitly mocked in tests.
        tracing::info!(%connector, "using non-default calendar connector");
    }

    let material = fetch_meeting_material(
        &connector,
        &req.calendar_event_id,
        req.title.as_deref(),
        req.transcript.as_deref(),
        &request_id,
    )
    .await?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::MeetingSummary, id).as_str();
    let action_items = serde_json::to_value(&material.action_items).unwrap_or(json!([]));
    let created_at = Utc::now();

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_meeting_summary (
            id, org_id, public_id, calendar_event_id, calendar_connector,
            transcript, summary_markdown, action_items, status, created_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'suggested',$9)
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(&public_id)
    .bind(&material.calendar_event_id)
    .bind(&material.connector_key)
    .bind(&material.transcript)
    .bind(&material.summary_markdown)
    .bind(&action_items)
    .bind(created_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(Json(MeetingSummaryView {
        id: id.to_string(),
        public_id,
        calendar_event_id: material.calendar_event_id,
        calendar_connector: material.connector_key,
        transcript: Some(material.transcript),
        summary_markdown: material.summary_markdown,
        action_items,
        status: "suggested".into(),
        accepted_at: None,
        accepted_by: None,
        created_at,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/meeting-summaries/{id}/accept",
    responses((status = 200, body = MeetingSummaryView)),
    tag = "ai"
)]
pub async fn accept_meeting_summary(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<MeetingSummaryView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_meeting_summary_accept(), &request_id)?;

    let uuid = resolve_summary_uuid(&id, &request_id)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT public_id, status FROM ai_meeting_summary WHERE id = $1 AND org_id = $2",
    )
    .bind(uuid)
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some((public_id, status)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "meeting summary not found",
        ));
    };
    if status != "suggested" {
        return Err(AppError::new(
            ErrorCode::Conflict,
            &request_id,
            format!("meeting summary status is {status}, expected suggested"),
        ));
    }

    let accepted_at = Utc::now();
    sqlx::query(
        r#"
        UPDATE ai_meeting_summary
        SET status = 'accepted', accepted_at = $3, accepted_by = $4
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(uuid)
    .bind(org_id.as_uuid())
    .bind(accepted_at)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id.as_uuid(),
        user_id,
        user_id,
        false,
        "ai.meeting_summary.accept",
        "meeting_summary",
        &public_id,
        json!({
            "status": "accepted",
            "note": "Human accepted summary; tasks are not auto-created",
        }),
    )
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let view = load_summary(&state, org_id.as_uuid(), &id, &request_id).await?;
    Ok(Json(view))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/meeting-summaries/{id}/reject",
    responses((status = 200, body = MessageResponse)),
    tag = "ai"
)]
pub async fn reject_meeting_summary(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_meeting_summary_accept(), &request_id)?;

    let uuid = resolve_summary_uuid(&id, &request_id)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT public_id, status FROM ai_meeting_summary WHERE id = $1 AND org_id = $2",
    )
    .bind(uuid)
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some((public_id, status)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "meeting summary not found",
        ));
    };
    if status != "suggested" {
        return Err(AppError::new(
            ErrorCode::Conflict,
            &request_id,
            format!("meeting summary status is {status}, expected suggested"),
        ));
    }

    sqlx::query(
        "UPDATE ai_meeting_summary SET status = 'rejected' WHERE id = $1 AND org_id = $2",
    )
    .bind(uuid)
    .bind(org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id.as_uuid(),
        user_id,
        user_id,
        false,
        "ai.meeting_summary.reject",
        "meeting_summary",
        &public_id,
        json!({ "status": "rejected" }),
    )
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(Json(MessageResponse {
        message: "meeting summary rejected".into(),
    }))
}

fn resolve_summary_uuid(id: &str, request_id: &str) -> Result<Uuid, AppError> {
    if let Ok(u) = Uuid::parse_str(id) {
        return Ok(u);
    }
    if let Ok(pid) = id.parse::<PublicId>() {
        if pid.kind() == IdKind::MeetingSummary {
            return Ok(pid.uuid());
        }
    }
    Err(AppError::new(
        ErrorCode::ValidationFailed,
        request_id,
        "invalid meeting summary id",
    ))
}

type SummaryRow = (
    Uuid,
    String,
    String,
    String,
    Option<String>,
    String,
    serde_json::Value,
    String,
    Option<chrono::DateTime<Utc>>,
    Option<Uuid>,
    chrono::DateTime<Utc>,
);

fn row_to_view(r: SummaryRow) -> MeetingSummaryView {
    let (
        id,
        public_id,
        calendar_event_id,
        calendar_connector,
        transcript,
        summary_markdown,
        action_items,
        status,
        accepted_at,
        accepted_by,
        created_at,
    ) = r;
    MeetingSummaryView {
        id: id.to_string(),
        public_id,
        calendar_event_id,
        calendar_connector,
        transcript,
        summary_markdown,
        action_items,
        status,
        accepted_at,
        accepted_by: accepted_by.map(|u| PublicId::new(IdKind::User, u).as_str()),
        created_at,
    }
}

async fn load_summary(
    state: &AppState,
    org_uuid: Uuid,
    id: &str,
    request_id: &str,
) -> Result<MeetingSummaryView, AppError> {
    let uuid = resolve_summary_uuid(id, request_id)?;
    let org_id = companyos_tenancy::OrgId::new(org_uuid);

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let row: Option<SummaryRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, calendar_event_id, calendar_connector, transcript,
               summary_markdown, action_items, status, accepted_at, accepted_by, created_at
        FROM ai_meeting_summary
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(uuid)
    .bind(org_uuid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    row.map(row_to_view).ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, request_id, "meeting summary not found")
    })
}
