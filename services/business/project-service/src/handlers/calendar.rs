//! `/api/v1/operations/calendar` — tasks with `due_at` in a date range.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{internal, parse_user_ref, user_public, validation};
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{CalendarEventDto, CalendarResponse, ListQuery};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/operations/calendar", get(get_calendar))
}

fn parse_range_bound(raw: &str, field: &str, request_id: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| validation(request_id, format!("{field} must be RFC3339")))
}

#[derive(Debug, sqlx::FromRow)]
struct CalRow {
    public_id: String,
    title: String,
    due_at: DateTime<Utc>,
    status: String,
    project_id: Uuid,
    assignee_id: Option<Uuid>,
}

/// GET /api/v1/operations/calendar
#[utoipa::path(get, path = "/api/v1/operations/calendar", tag = "operations-calendar",
    params(
        ("from" = String, Query),
        ("to" = String, Query),
    ),
    responses((status = 200, body = CalendarResponse)))]
pub async fn get_calendar(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<CalendarResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let from_raw = q
        .from
        .as_deref()
        .ok_or_else(|| validation(&request_id, "from is required"))?;
    let to_raw =
        q.to.as_deref()
            .ok_or_else(|| validation(&request_id, "to is required"))?;
    let from = parse_range_bound(from_raw, "from", &request_id)?;
    let to = parse_range_bound(to_raw, "to", &request_id)?;
    if to < from {
        return Err(validation(&request_id, "to must be >= from"));
    }

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_task_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::operations_task_read());

    let assignee_id = match q.assignee_id.as_deref() {
        Some(s) => Some(parse_user_ref(s, &request_id)?),
        None => None,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        r#"
        SELECT public_id, title, due_at, status, project_id, assignee_id
        FROM operations_task
        WHERE org_id = 
        "#,
    );
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL AND due_at IS NOT NULL AND due_at >= ");
    qb.push_bind(from);
    qb.push(" AND due_at < ");
    qb.push_bind(to);
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    if let Some(aid) = assignee_id {
        qb.push(" AND assignee_id = ");
        qb.push_bind(aid);
    }
    qb.push(" ORDER BY due_at ASC");

    let rows: Vec<CalRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    let events = rows
        .into_iter()
        .map(|r| CalendarEventDto {
            id: r.public_id,
            title: r.title,
            due_at: r.due_at.to_rfc3339(),
            status: r.status,
            project_id: PublicId::new(IdKind::Project, r.project_id).as_str(),
            assignee_id: r.assignee_id.map(user_public),
        })
        .collect();

    Ok(Json(CalendarResponse { events }))
}
