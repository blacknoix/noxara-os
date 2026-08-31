//! `/api/v1/operations/timesheets` — week timesheets + time entries.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use companyos_authz::{perms, Role};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{
    conflict, internal, normalize_paging, not_found, parse_public_id, parse_user_ref, user_public,
    validation,
};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope, MembershipScope};
use crate::state::AppState;
use crate::types::{
    CreateTimesheetRequest, DecideTimesheetRequest, PatchTimeEntryRequest, TimeEntryDto,
    TimesheetDto, TimesheetListQuery, TimesheetListResponse, UpsertTimeEntryRequest,
    TIMESHEET_STATUSES,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/operations/timesheets",
            get(list_timesheets).post(create_timesheet),
        )
        .route(
            "/api/v1/operations/timesheets/{id}",
            get(get_timesheet),
        )
        .route(
            "/api/v1/operations/timesheets/{id}/entries",
            post(upsert_entry),
        )
        .route(
            "/api/v1/operations/timesheets/{id}/entries/{entry_id}",
            axum::routing::patch(patch_entry),
        )
        .route(
            "/api/v1/operations/timesheets/{id}/submit",
            post(submit_timesheet),
        )
        .route(
            "/api/v1/operations/timesheets/{id}/approve",
            post(approve_timesheet),
        )
        .route(
            "/api/v1/operations/timesheets/{id}/reject",
            post(reject_timesheet),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct TimesheetRow {
    id: Uuid,
    public_id: String,
    membership_user_id: Uuid,
    week_start: NaiveDate,
    status: String,
    submitted_at: Option<DateTime<Utc>>,
    approved_at: Option<DateTime<Utc>>,
    approved_by: Option<Uuid>,
    approval_id: Option<String>,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct EntryRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    membership_user_id: Uuid,
    project_id: Uuid,
    project_public_id: String,
    task_id: Option<Uuid>,
    task_public_id: Option<String>,
    entry_date: NaiveDate,
    minutes: i32,
    billable: bool,
    notes: Option<String>,
    timesheet_id: Option<Uuid>,
    timesheet_public_id: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const TS_COLS: &str = r#"
    id, public_id, membership_user_id, week_start, status,
    submitted_at, approved_at, approved_by, approval_id, notes,
    created_at, updated_at, version
"#;

const ENTRY_COLS: &str = r#"
    e.id, e.public_id, e.membership_user_id, e.project_id, p.public_id AS project_public_id,
    e.task_id, t.public_id AS task_public_id, e.entry_date, e.minutes, e.billable, e.notes,
    e.timesheet_id, ts.public_id AS timesheet_public_id, e.status,
    e.created_at, e.updated_at, e.version
"#;

fn monday_of(date: NaiveDate) -> NaiveDate {
    let days = date.weekday().num_days_from_monday() as i64;
    date - Duration::days(days)
}

fn parse_date(raw: &str, field: &str, request_id: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| validation(request_id, format!("{field} must be YYYY-MM-DD")))
}

fn validate_status(status: &str, request_id: &str) -> Result<(), AppError> {
    if TIMESHEET_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(validation(
            request_id,
            format!("status must be one of: {}", TIMESHEET_STATUSES.join("|")),
        ))
    }
}

fn entry_to_dto(row: EntryRow) -> TimeEntryDto {
    TimeEntryDto {
        id: row.public_id,
        membership_user_id: user_public(row.membership_user_id),
        project_id: row.project_public_id,
        task_id: row.task_public_id,
        entry_date: row.entry_date.to_string(),
        minutes: row.minutes,
        billable: row.billable,
        notes: row.notes,
        timesheet_id: row.timesheet_public_id,
        status: row.status,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        version: row.version,
    }
}

fn timesheet_to_dto(row: TimesheetRow, entries: Vec<TimeEntryDto>) -> TimesheetDto {
    TimesheetDto {
        id: row.public_id,
        membership_user_id: user_public(row.membership_user_id),
        week_start: row.week_start.to_string(),
        status: row.status,
        submitted_at: row.submitted_at.map(|t| t.to_rfc3339()),
        approved_at: row.approved_at.map(|t| t.to_rfc3339()),
        approved_by: row.approved_by.map(user_public),
        approval_id: row.approval_id,
        notes: row.notes,
        entries,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        version: row.version,
    }
}

fn can_manage_others(membership: &MembershipScope) -> bool {
    membership.principal.roles.iter().any(|r| {
        matches!(r, Role::Owner | Role::Admin | Role::Manager)
    }) || companyos_authz::decide(
        &membership.principal,
        &perms::operations_timesheet_approve(),
    )
    .decision
        == companyos_authz::Decision::Allow
}

fn can_self_approve(membership: &MembershipScope) -> bool {
    membership
        .principal
        .roles
        .iter()
        .any(|r| matches!(r, Role::Owner | Role::Admin | Role::Manager))
}

async fn fetch_timesheet(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    id: Uuid,
) -> Result<Option<TimesheetRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {TS_COLS} FROM operations_timesheet WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
}

async fn fetch_entries_for_timesheet(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    timesheet_id: Uuid,
) -> Result<Vec<EntryRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {ENTRY_COLS}
         FROM operations_time_entry e
         JOIN operations_project p ON p.id = e.project_id AND p.org_id = e.org_id
         LEFT JOIN operations_task t ON t.id = e.task_id AND t.org_id = e.org_id
         LEFT JOIN operations_timesheet ts ON ts.id = e.timesheet_id AND ts.org_id = e.org_id
         WHERE e.org_id = $1 AND e.timesheet_id = $2
         ORDER BY e.entry_date, e.created_at"
    ))
    .bind(org_id)
    .bind(timesheet_id)
    .fetch_all(&mut **tx)
    .await
}

async fn fetch_entry(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    entry_id: Uuid,
) -> Result<Option<EntryRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {ENTRY_COLS}
         FROM operations_time_entry e
         JOIN operations_project p ON p.id = e.project_id AND p.org_id = e.org_id
         LEFT JOIN operations_task t ON t.id = e.task_id AND t.org_id = e.org_id
         LEFT JOIN operations_timesheet ts ON ts.id = e.timesheet_id AND ts.org_id = e.org_id
         WHERE e.org_id = $1 AND e.id = $2"
    ))
    .bind(org_id)
    .bind(entry_id)
    .fetch_optional(&mut **tx)
    .await
}

fn assert_can_read_sheet(
    auth: &AuthCtx,
    membership: &MembershipScope,
    sheet_user: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    enforce_any_scope(
        &membership.principal,
        perms::operations_timesheet_read(),
        request_id,
    )?;
    if sheet_user == auth.ctx.actor.user_id || can_manage_others(membership) {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "cannot read another member's timesheet",
        ))
    }
}

fn assert_can_edit_draft(
    auth: &AuthCtx,
    membership: &MembershipScope,
    sheet: &TimesheetRow,
    request_id: &str,
) -> Result<(), AppError> {
    enforce_any_scope(
        &membership.principal,
        perms::operations_timesheet_write(),
        request_id,
    )?;
    let is_owner = sheet.membership_user_id == auth.ctx.actor.user_id;
    if sheet.status == "draft" {
        if is_owner || can_manage_others(membership) {
            return Ok(());
        }
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "cannot edit another member's draft timesheet",
        ));
    }
    // Submitted/approved/rejected: only approvers may mutate another member's sheet.
    if is_owner {
        return Err(conflict(
            request_id,
            format!("timesheet is {} and cannot be edited by owner", sheet.status),
        ));
    }
    enforce_any_scope(
        &membership.principal,
        perms::operations_timesheet_approve(),
        request_id,
    )?;
    Ok(())
}

async fn resolve_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    project_public: &str,
    request_id: &str,
) -> Result<Uuid, AppError> {
    let pid = parse_public_id(IdKind::Project, project_public, request_id)?;
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM operations_project WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .bind(pid)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    exists
        .map(|r| r.0)
        .ok_or_else(|| not_found(request_id, "project"))
}

async fn resolve_task(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    project_id: Uuid,
    task_public: Option<&str>,
    request_id: &str,
) -> Result<Option<Uuid>, AppError> {
    let Some(raw) = task_public.filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let tid = parse_public_id(IdKind::Task, raw, request_id)?;
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM operations_task
         WHERE org_id = $1 AND id = $2 AND project_id = $3 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .bind(tid)
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    row.map(|r| r.0)
        .map(Some)
        .ok_or_else(|| not_found(request_id, "task"))
}

fn week_end(week_start: NaiveDate) -> NaiveDate {
    week_start + Duration::days(6)
}

fn date_in_week(date: NaiveDate, week_start: NaiveDate, request_id: &str) -> Result<(), AppError> {
    let end = week_end(week_start);
    if date < week_start || date > end {
        return Err(validation(
            request_id,
            format!("entry_date must fall within the week {week_start}..={end}"),
        ));
    }
    Ok(())
}

/// GET /api/v1/operations/timesheets
#[utoipa::path(get, path = "/api/v1/operations/timesheets", tag = "operations-timesheets",
    params(TimesheetListQuery),
    responses((status = 200, body = TimesheetListResponse)))]
pub async fn list_timesheets(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<TimesheetListQuery>,
) -> Result<Json<TimesheetListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_timesheet_read(),
        &request_id,
    )?;

    let filter_user = match q.membership_user_id.as_deref() {
        Some(s) => {
            let uid = parse_user_ref(s, &request_id)?;
            if uid != auth.ctx.actor.user_id && !can_manage_others(&membership) {
                return Err(AppError::new(
                    ErrorCode::Forbidden,
                    &request_id,
                    "cannot list another member's timesheets",
                ));
            }
            Some(uid)
        }
        None if can_manage_others(&membership) => None,
        None => Some(auth.ctx.actor.user_id),
    };

    if let Some(status) = q.status.as_deref() {
        validate_status(status, &request_id)?;
    }
    let from = q
        .from
        .as_deref()
        .map(|s| parse_date(s, "from", &request_id))
        .transpose()?;
    let to = q
        .to
        .as_deref()
        .map(|s| parse_date(s, "to", &request_id))
        .transpose()?;
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint FROM operations_timesheet
        WHERE org_id = $1
          AND ($2::uuid IS NULL OR membership_user_id = $2)
          AND ($3::text IS NULL OR status = $3)
          AND ($4::date IS NULL OR week_start >= $4)
          AND ($5::date IS NULL OR week_start <= $5)
        "#,
    )
    .bind(org_id)
    .bind(filter_user)
    .bind(q.status.as_deref())
    .bind(from)
    .bind(to)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let rows: Vec<TimesheetRow> = sqlx::query_as(&format!(
        r#"
        SELECT {TS_COLS} FROM operations_timesheet
        WHERE org_id = $1
          AND ($2::uuid IS NULL OR membership_user_id = $2)
          AND ($3::text IS NULL OR status = $3)
          AND ($4::date IS NULL OR week_start >= $4)
          AND ($5::date IS NULL OR week_start <= $5)
        ORDER BY week_start DESC
        LIMIT $6 OFFSET $7
        "#
    ))
    .bind(org_id)
    .bind(filter_user)
    .bind(q.status.as_deref())
    .bind(from)
    .bind(to)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let entries = fetch_entries_for_timesheet(&mut tx, org_id, row.id)
            .await
            .map_err(internal(&request_id))?;
        items.push(timesheet_to_dto(
            row,
            entries.into_iter().map(entry_to_dto).collect(),
        ));
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(TimesheetListResponse { items, total }))
}

/// POST /api/v1/operations/timesheets
#[utoipa::path(post, path = "/api/v1/operations/timesheets", tag = "operations-timesheets",
    request_body = CreateTimesheetRequest,
    responses((status = 201, body = TimesheetDto)))]
pub async fn create_timesheet(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateTimesheetRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_timesheet_write(),
        &request_id,
    )?;

    let week_start = monday_of(parse_date(&body.week_start, "week_start", &request_id)?);
    let target_user = match body.membership_user_id.as_deref() {
        Some(s) => {
            let uid = parse_user_ref(s, &request_id)?;
            if uid != auth.ctx.actor.user_id && !can_manage_others(&membership) {
                return Err(AppError::new(
                    ErrorCode::Forbidden,
                    &request_id,
                    "cannot create timesheet for another member",
                ));
            }
            uid
        }
        None => auth.ctx.actor.user_id,
    };

    let public_id = PublicId::generate(IdKind::Timesheet);
    let id = public_id.uuid();
    let idem_key = idempotency::header_key(&headers);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "timesheet.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    // Idempotent on unique (org, user, week): return existing.
    if let Some(existing) = sqlx::query_as::<_, TimesheetRow>(&format!(
        "SELECT {TS_COLS} FROM operations_timesheet
         WHERE org_id = $1 AND membership_user_id = $2 AND week_start = $3"
    ))
    .bind(org_id)
    .bind(target_user)
    .bind(week_start)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    {
        let entries = fetch_entries_for_timesheet(&mut tx, org_id, existing.id)
            .await
            .map_err(internal(&request_id))?;
        let dto = timesheet_to_dto(
            existing,
            entries.into_iter().map(entry_to_dto).collect(),
        );
        let body = serde_json::to_value(&dto).unwrap_or_default();
        if let Some(key) = idem_key.as_deref() {
            let _ = idempotency::put(&mut *tx, org_id, "timesheet.create", key, 200, body.clone())
                .await;
        }
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((StatusCode::OK, Json(body)).into_response());
    }

    sqlx::query(
        r#"
        INSERT INTO operations_timesheet (
            id, org_id, public_id, membership_user_id, week_start, status, notes
        ) VALUES ($1,$2,$3,$4,$5,'draft',$6)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(target_user)
    .bind(week_start)
    .bind(body.notes.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_timesheet(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "timesheet"))?;
    let dto = timesheet_to_dto(row, vec![]);

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.timesheet.create",
        "timesheet",
        &dto.id,
        serde_json::json!({ "week_start": week_start.to_string() }),
    )
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "timesheet",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "week_start": week_start.to_string() }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let body_val = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "timesheet.create",
            key,
            201,
            body_val.clone(),
        )
        .await
        .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(body_val)).into_response())
}

/// GET /api/v1/operations/timesheets/{id}
#[utoipa::path(get, path = "/api/v1/operations/timesheets/{id}", tag = "operations-timesheets",
    responses((status = 200, body = TimesheetDto)))]
pub async fn get_timesheet(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<TimesheetDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    let tid = parse_public_id(IdKind::Timesheet, &id, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "timesheet"))?;
    assert_can_read_sheet(&auth, &membership, row.membership_user_id, &request_id)?;
    let entries = fetch_entries_for_timesheet(&mut tx, org_id, row.id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(timesheet_to_dto(
        row,
        entries.into_iter().map(entry_to_dto).collect(),
    )))
}

/// POST /api/v1/operations/timesheets/{id}/entries
#[utoipa::path(post, path = "/api/v1/operations/timesheets/{id}/entries", tag = "operations-timesheets",
    request_body = UpsertTimeEntryRequest,
    responses((status = 200, body = TimeEntryDto), (status = 201, body = TimeEntryDto)))]
pub async fn upsert_entry(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpsertTimeEntryRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    let tid = parse_public_id(IdKind::Timesheet, &id, &request_id)?;

    if body.minutes <= 0 {
        return Err(validation(&request_id, "minutes must be > 0"));
    }
    let entry_date = parse_date(&body.entry_date, "entry_date", &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let sheet = fetch_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "timesheet"))?;
    assert_can_edit_draft(&auth, &membership, &sheet, &request_id)?;
    date_in_week(entry_date, sheet.week_start, &request_id)?;

    let project_id = resolve_project(&mut tx, org_id, &body.project_id, &request_id).await?;
    let task_id =
        resolve_task(&mut tx, org_id, project_id, body.task_id.as_deref(), &request_id).await?;
    let billable = body.billable.unwrap_or(true);

    let (status_code, entry) = if let Some(raw_id) = body.id.as_deref() {
        let eid = parse_public_id(IdKind::TimeEntry, raw_id, &request_id)?;
        let existing = fetch_entry(&mut tx, org_id, eid)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "time entry"))?;
        if existing.timesheet_id != Some(sheet.id) {
            return Err(validation(
                &request_id,
                "entry does not belong to this timesheet",
            ));
        }
        if existing.status != "draft" && sheet.status != "draft" {
            return Err(conflict(&request_id, "only draft entries can be updated"));
        }
        sqlx::query(
            r#"
            UPDATE operations_time_entry SET
                project_id = $3, task_id = $4, entry_date = $5, minutes = $6,
                billable = $7, notes = $8, updated_at = now(), version = version + 1
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(eid)
        .bind(project_id)
        .bind(task_id)
        .bind(entry_date)
        .bind(body.minutes)
        .bind(billable)
        .bind(body.notes.as_deref())
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        let updated = fetch_entry(&mut tx, org_id, eid)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "time entry"))?;
        (StatusCode::OK, updated)
    } else {
        let public_id = PublicId::generate(IdKind::TimeEntry);
        let eid = public_id.uuid();
        sqlx::query(
            r#"
            INSERT INTO operations_time_entry (
                id, org_id, public_id, membership_user_id, project_id, task_id,
                entry_date, minutes, billable, notes, timesheet_id, status
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'draft')
            "#,
        )
        .bind(eid)
        .bind(org_id)
        .bind(public_id.as_str())
        .bind(sheet.membership_user_id)
        .bind(project_id)
        .bind(task_id)
        .bind(entry_date)
        .bind(body.minutes)
        .bind(billable)
        .bind(body.notes.as_deref())
        .bind(sheet.id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        let created = fetch_entry(&mut tx, org_id, eid)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "time entry"))?;
        (StatusCode::CREATED, created)
    };

    let dto = entry_to_dto(entry);
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.time_entry.upsert",
        "time_entry",
        &dto.id,
        serde_json::json!({ "timesheet_id": id, "minutes": dto.minutes }),
    )
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((status_code, Json(dto)).into_response())
}

/// PATCH /api/v1/operations/timesheets/{id}/entries/{entry_id}
#[utoipa::path(patch, path = "/api/v1/operations/timesheets/{id}/entries/{entry_id}",
    tag = "operations-timesheets",
    request_body = PatchTimeEntryRequest,
    responses((status = 200, body = TimeEntryDto)))]
pub async fn patch_entry(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path((id, entry_id)): Path<(String, String)>,
    Json(body): Json<PatchTimeEntryRequest>,
) -> Result<Json<TimeEntryDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    let tid = parse_public_id(IdKind::Timesheet, &id, &request_id)?;
    let eid = parse_public_id(IdKind::TimeEntry, &entry_id, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let sheet = fetch_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "timesheet"))?;
    assert_can_edit_draft(&auth, &membership, &sheet, &request_id)?;

    let existing = fetch_entry(&mut tx, org_id, eid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "time entry"))?;
    if existing.timesheet_id != Some(sheet.id) {
        return Err(validation(
            &request_id,
            "entry does not belong to this timesheet",
        ));
    }
    if existing.status != "draft" && sheet.status != "draft" {
        return Err(conflict(&request_id, "only draft entries can be patched"));
    }
    // Owner-only for patch when not a manager editing.
    if existing.membership_user_id != auth.ctx.actor.user_id && !can_manage_others(&membership) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            &request_id,
            "only the entry owner can patch a draft entry",
        ));
    }

    let project_id = if let Some(ref p) = body.project_id {
        resolve_project(&mut tx, org_id, p, &request_id).await?
    } else {
        existing.project_id
    };
    let task_id = if body.task_id.is_some() {
        resolve_task(
            &mut tx,
            org_id,
            project_id,
            body.task_id.as_deref(),
            &request_id,
        )
        .await?
    } else {
        existing.task_id
    };
    let entry_date = match body.entry_date.as_deref() {
        Some(s) => {
            let d = parse_date(s, "entry_date", &request_id)?;
            date_in_week(d, sheet.week_start, &request_id)?;
            d
        }
        None => existing.entry_date,
    };
    let minutes = match body.minutes {
        Some(m) if m > 0 => m,
        Some(_) => return Err(validation(&request_id, "minutes must be > 0")),
        None => existing.minutes,
    };
    let billable = body.billable.unwrap_or(existing.billable);
    let notes = body.notes.clone().or(existing.notes.clone());

    sqlx::query(
        r#"
        UPDATE operations_time_entry SET
            project_id = $3, task_id = $4, entry_date = $5, minutes = $6,
            billable = $7, notes = $8, updated_at = now(), version = version + 1
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(eid)
    .bind(project_id)
    .bind(task_id)
    .bind(entry_date)
    .bind(minutes)
    .bind(billable)
    .bind(notes.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let updated = fetch_entry(&mut tx, org_id, eid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "time entry"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(entry_to_dto(updated)))
}

/// POST /api/v1/operations/timesheets/{id}/submit
#[utoipa::path(post, path = "/api/v1/operations/timesheets/{id}/submit", tag = "operations-timesheets",
    responses((status = 200, body = TimesheetDto)))]
pub async fn submit_timesheet(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<TimesheetDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_timesheet_submit(),
        &request_id,
    )?;
    let tid = parse_public_id(IdKind::Timesheet, &id, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let sheet = fetch_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "timesheet"))?;

    let is_owner = sheet.membership_user_id == auth.ctx.actor.user_id;
    if !is_owner && !can_manage_others(&membership) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            &request_id,
            "can only submit own timesheet unless manager",
        ));
    }
    if sheet.status != "draft" && sheet.status != "rejected" {
        return Err(conflict(
            &request_id,
            format!("timesheet is {} and cannot be submitted", sheet.status),
        ));
    }

    // v1: mark submitted; dedicated approve/reject endpoints (no Temporal required).
    // approval_id left null unless a future path starts ApprovalProcess.
    sqlx::query(
        r#"
        UPDATE operations_timesheet SET
            status = 'submitted', submitted_at = now(),
            approved_at = NULL, approved_by = NULL, approval_id = NULL,
            updated_at = now(), version = version + 1
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(tid)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    sqlx::query(
        r#"
        UPDATE operations_time_entry SET
            status = 'submitted', updated_at = now(), version = version + 1
        WHERE org_id = $1 AND timesheet_id = $2 AND status = 'draft'
        "#,
    )
    .bind(org_id)
    .bind(tid)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "timesheet"))?;
    let entries = fetch_entries_for_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?;
    let dto = timesheet_to_dto(row, entries.into_iter().map(entry_to_dto).collect());

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.timesheet.submit",
        "timesheet",
        &dto.id,
        serde_json::json!({ "status": "submitted" }),
    )
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "timesheet",
        "submitted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "status": "submitted" }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

async fn decide_timesheet(
    state: &AppState,
    auth: AuthCtx,
    id: &str,
    approve: bool,
    note: Option<String>,
) -> Result<Json<TimesheetDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_timesheet_approve(),
        &request_id,
    )?;
    let tid = parse_public_id(IdKind::Timesheet, id, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let sheet = fetch_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "timesheet"))?;

    if sheet.membership_user_id == auth.ctx.actor.user_id && !can_self_approve(&membership) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            &request_id,
            "cannot approve own timesheet without being manager",
        ));
    }
    if sheet.status != "submitted" {
        return Err(conflict(
            &request_id,
            format!("timesheet is {} and cannot be decided", sheet.status),
        ));
    }

    let new_status = if approve { "approved" } else { "rejected" };
    sqlx::query(
        r#"
        UPDATE operations_timesheet SET
            status = $3,
            approved_at = CASE WHEN $3 = 'approved' THEN now() ELSE NULL END,
            approved_by = $4,
            notes = COALESCE($5, notes),
            updated_at = now(), version = version + 1
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(tid)
    .bind(new_status)
    .bind(auth.ctx.actor.user_id)
    .bind(note.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    sqlx::query(
        r#"
        UPDATE operations_time_entry SET
            status = $3, updated_at = now(), version = version + 1
        WHERE org_id = $1 AND timesheet_id = $2 AND status = 'submitted'
        "#,
    )
    .bind(org_id)
    .bind(tid)
    .bind(new_status)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "timesheet"))?;
    let entries = fetch_entries_for_timesheet(&mut tx, org_id, tid)
        .await
        .map_err(internal(&request_id))?;
    let dto = timesheet_to_dto(row, entries.into_iter().map(entry_to_dto).collect());

    let action = if approve {
        "operations.timesheet.approve"
    } else {
        "operations.timesheet.reject"
    };
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        action,
        "timesheet",
        &dto.id,
        serde_json::json!({ "status": new_status }),
    )
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "timesheet",
        if approve { "approved" } else { "rejected" },
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "status": new_status }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/operations/timesheets/{id}/approve
#[utoipa::path(post, path = "/api/v1/operations/timesheets/{id}/approve", tag = "operations-timesheets",
    request_body = DecideTimesheetRequest,
    responses((status = 200, body = TimesheetDto)))]
pub async fn approve_timesheet(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    body: Option<Json<DecideTimesheetRequest>>,
) -> Result<Json<TimesheetDto>, AppError> {
    let note = body.and_then(|Json(b)| b.note);
    decide_timesheet(&state, auth, &id, true, note).await
}

/// POST /api/v1/operations/timesheets/{id}/reject
#[utoipa::path(post, path = "/api/v1/operations/timesheets/{id}/reject", tag = "operations-timesheets",
    request_body = DecideTimesheetRequest,
    responses((status = 200, body = TimesheetDto)))]
pub async fn reject_timesheet(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    body: Option<Json<DecideTimesheetRequest>>,
) -> Result<Json<TimesheetDto>, AppError> {
    let note = body.and_then(|Json(b)| b.note);
    decide_timesheet(&state, auth, &id, false, note).await
}
