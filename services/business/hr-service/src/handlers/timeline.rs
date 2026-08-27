//! `/api/v1/people/employees/{id}/timeline` — HR activity feed.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::IdKind;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::employees::{enforce_employee_scope, fetch_employee_row};
use super::{internal, not_found, parse_public_id, user_public};
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{TimelineEventDto, TimelineResponse};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/people/employees/{id}/timeline",
        get(get_timeline),
    )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct TimelineRow {
    id: Uuid,
    event_type: String,
    summary: String,
    metadata: serde_json::Value,
    occurred_at: DateTime<Utc>,
    actor_user_id: Option<Uuid>,
}

/// GET /api/v1/people/employees/{id}/timeline
#[utoipa::path(
    get,
    path = "/api/v1/people/employees/{id}/timeline",
    tag = "people-timeline",
    params(("id" = String, Path)),
    responses((status = 200, body = TimelineResponse), (status = 404))
)]
pub async fn get_timeline(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<TimelineResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_employee_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_read(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let rows: Vec<TimelineRow> = sqlx::query_as(
        r#"
        SELECT id, event_type, summary, metadata, occurred_at, actor_user_id
        FROM people_timeline_event
        WHERE org_id = $1 AND employee_id = $2
        ORDER BY occurred_at DESC
        LIMIT 200
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(TimelineResponse {
        items: rows
            .into_iter()
            .map(|r| TimelineEventDto {
                id: r.id.to_string(),
                event_type: r.event_type,
                summary: r.summary,
                metadata: r.metadata,
                occurred_at: r.occurred_at.to_rfc3339(),
                actor_user_id: r.actor_user_id.map(user_public),
            })
            .collect(),
    }))
}
