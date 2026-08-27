//! `/api/v1/people/schedules` and `/api/v1/people/holidays`.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::Deserialize;

use super::{internal, normalize_paging, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreateHolidayRequest, CreateWorkScheduleRequest, HolidayDto, HolidayListResponse,
    WorkScheduleDto, WorkScheduleListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/people/schedules",
            get(list_schedules).post(create_schedule),
        )
        .route("/api/v1/people/schedules/{id}", get(get_schedule))
        .route(
            "/api/v1/people/holidays",
            get(list_holidays).post(create_holiday),
        )
        .route("/api/v1/people/holidays/{id}", get(get_holiday))
}

#[derive(Debug, Deserialize, Default)]
pub struct HolidayQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub location: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ScheduleRow {
    public_id: String,
    name: String,
    timezone: String,
    weekly_hours: serde_json::Value,
    location: Option<String>,
    is_default: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl ScheduleRow {
    fn into_dto(self) -> WorkScheduleDto {
        WorkScheduleDto {
            id: self.public_id,
            name: self.name,
            timezone: self.timezone,
            weekly_hours: self.weekly_hours,
            location: self.location,
            is_default: self.is_default,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct HolidayRow {
    public_id: String,
    name: String,
    holiday_date: NaiveDate,
    location: Option<String>,
    is_half_day: bool,
    half_day_period: Option<String>,
    created_at: DateTime<Utc>,
    version: i32,
}

impl HolidayRow {
    fn into_dto(self) -> HolidayDto {
        HolidayDto {
            id: self.public_id,
            name: self.name,
            holiday_date: self.holiday_date.to_string(),
            location: self.location,
            is_half_day: self.is_half_day,
            half_day_period: self.half_day_period,
            created_at: self.created_at.to_rfc3339(),
            version: self.version,
        }
    }
}

/// GET /api/v1/people/schedules
#[utoipa::path(get, path = "/api/v1/people/schedules", tag = "people-schedules",
    responses((status = 200, body = WorkScheduleListResponse)))]
pub async fn list_schedules(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<WorkScheduleListResponse>, AppError> {
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
        perms::hr_attendance_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let rows: Vec<ScheduleRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, timezone, weekly_hours, location, is_default,
               created_at, updated_at, version
        FROM people_work_schedule
        WHERE org_id = $1 AND deleted_at IS NULL
        ORDER BY is_default DESC, name ASC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(WorkScheduleListResponse {
        items: rows.into_iter().map(ScheduleRow::into_dto).collect(),
    }))
}

/// POST /api/v1/people/schedules
#[utoipa::path(post, path = "/api/v1/people/schedules", tag = "people-schedules",
    request_body = CreateWorkScheduleRequest,
    responses((status = 201, body = WorkScheduleDto)))]
pub async fn create_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateWorkScheduleRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name is required"));
    }
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_attendance_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) = idempotency::get(&mut *tx, org_id, "schedule.create", &key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            return Ok((
                StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK),
                Json(cached),
            )
                .into_response());
        }
    }

    let public_id = PublicId::generate(IdKind::WorkSchedule);
    let id = public_id.uuid();
    let tz = body.timezone.as_deref().unwrap_or("UTC").trim().to_string();
    let weekly = body.weekly_hours.unwrap_or_else(|| serde_json::json!({}));
    let is_default = body.is_default.unwrap_or(false);
    if is_default {
        sqlx::query(
            "UPDATE people_work_schedule SET is_default = false, updated_at = now()
             WHERE org_id = $1 AND deleted_at IS NULL AND is_default = true",
        )
        .bind(org_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let row: ScheduleRow = sqlx::query_as(
        r#"
        INSERT INTO people_work_schedule (
            id, org_id, public_id, name, timezone, weekly_hours, location,
            is_default, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING public_id, name, timezone, weekly_hours, location, is_default,
                  created_at, updated_at, version
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(&tz)
    .bind(&weekly)
    .bind(body.location.as_deref())
    .bind(is_default)
    .bind(auth.ctx.actor.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = row.into_dto();
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.schedule.create",
        "work_schedule",
        &dto.id,
        serde_json::json!({ "name": dto.name }),
    )
    .await
    .map_err(internal(&request_id))?;

    let body_json = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "schedule.create", &key, 201, body_json)
            .await
            .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// GET /api/v1/people/schedules/{id}
#[utoipa::path(get, path = "/api/v1/people/schedules/{id}", tag = "people-schedules",
    params(("id" = String, Path)),
    responses((status = 200, body = WorkScheduleDto), (status = 404)))]
pub async fn get_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<WorkScheduleDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let sid = parse_public_id(IdKind::WorkSchedule, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_attendance_read(),
        &request_id,
    )?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row: Option<ScheduleRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, timezone, weekly_hours, location, is_default,
               created_at, updated_at, version
        FROM people_work_schedule
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(sid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(
        row.ok_or_else(|| not_found(&request_id, "schedule"))?
            .into_dto(),
    ))
}

/// GET /api/v1/people/holidays
#[utoipa::path(get, path = "/api/v1/people/holidays", tag = "people-holidays",
    responses((status = 200, body = HolidayListResponse)))]
pub async fn list_holidays(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<HolidayQuery>,
) -> Result<Json<HolidayListResponse>, AppError> {
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
        perms::hr_attendance_read(),
        &request_id,
    )?;
    let (limit, offset) = normalize_paging(q.limit, q.offset);
    let from = parse_opt_date(q.from.as_deref(), &request_id)?;
    let to = parse_opt_date(q.to.as_deref(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let rows: Vec<HolidayRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, holiday_date, location, is_half_day, half_day_period,
               created_at, version
        FROM people_holiday
        WHERE org_id = $1 AND deleted_at IS NULL
          AND ($2::date IS NULL OR holiday_date >= $2)
          AND ($3::date IS NULL OR holiday_date <= $3)
          AND ($4::text IS NULL OR location = $4)
        ORDER BY holiday_date ASC
        LIMIT $5 OFFSET $6
        "#,
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .bind(q.location.as_deref())
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(HolidayListResponse {
        items: rows.into_iter().map(HolidayRow::into_dto).collect(),
    }))
}

/// POST /api/v1/people/holidays
#[utoipa::path(post, path = "/api/v1/people/holidays", tag = "people-holidays",
    request_body = CreateHolidayRequest,
    responses((status = 201, body = HolidayDto)))]
pub async fn create_holiday(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateHolidayRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name is required"));
    }
    let date = NaiveDate::parse_from_str(&body.holiday_date, "%Y-%m-%d")
        .map_err(|_| validation(&request_id, "holiday_date must be YYYY-MM-DD"))?;
    let is_half = body.is_half_day.unwrap_or(false);
    let period = body.half_day_period.as_deref();
    if is_half && !matches!(period, Some("am") | Some("pm")) {
        return Err(validation(
            &request_id,
            "half_day_period must be am or pm when is_half_day",
        ));
    }
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_attendance_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) = idempotency::get(&mut *tx, org_id, "holiday.create", &key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            return Ok((
                StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK),
                Json(cached),
            )
                .into_response());
        }
    }

    let public_id = PublicId::generate(IdKind::Holiday);
    let row: HolidayRow = sqlx::query_as(
        r#"
        INSERT INTO people_holiday (
            id, org_id, public_id, name, holiday_date, location,
            is_half_day, half_day_period, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING public_id, name, holiday_date, location, is_half_day, half_day_period,
                  created_at, version
        "#,
    )
    .bind(public_id.uuid())
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(date)
    .bind(body.location.as_deref())
    .bind(is_half)
    .bind(if is_half { period } else { None })
    .bind(auth.ctx.actor.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = row.into_dto();
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.holiday.create",
        "holiday",
        &dto.id,
        serde_json::json!({ "date": dto.holiday_date }),
    )
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "holiday",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "holiday_date": dto.holiday_date }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let body_json = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "holiday.create", &key, 201, body_json)
            .await
            .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// GET /api/v1/people/holidays/{id}
#[utoipa::path(get, path = "/api/v1/people/holidays/{id}", tag = "people-holidays",
    params(("id" = String, Path)),
    responses((status = 200, body = HolidayDto), (status = 404)))]
pub async fn get_holiday(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<HolidayDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let hid = parse_public_id(IdKind::Holiday, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_attendance_read(),
        &request_id,
    )?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row: Option<HolidayRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, holiday_date, location, is_half_day, half_day_period,
               created_at, version
        FROM people_holiday
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(hid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(
        row.ok_or_else(|| not_found(&request_id, "holiday"))?
            .into_dto(),
    ))
}

fn parse_opt_date(raw: Option<&str>, request_id: &str) -> Result<Option<NaiveDate>, AppError> {
    match raw {
        None | Some("") => Ok(None),
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| validation(request_id, format!("invalid date: {s}"))),
    }
}
