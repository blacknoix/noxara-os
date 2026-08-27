//! `/api/v1/people/attendance` — append-only attendance capture.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::employees::{
    enforce_employee_scope, fetch_employee_by_user, fetch_employee_row, EmployeeRow,
};
use super::{internal, normalize_paging, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    AttendanceDto, AttendanceImportRequest, AttendanceImportResponse, AttendanceListQuery,
    AttendanceListResponse, RecordAttendanceRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/people/attendance",
            get(list_attendance).post(record_attendance),
        )
        .route("/api/v1/people/attendance/import", post(import_attendance))
        .route("/api/v1/people/me/attendance", get(list_my_attendance))
}

const VALID_KINDS: &[&str] = &[
    "check_in",
    "check_out",
    "break_start",
    "break_end",
    "reversal",
    "adjustment",
];

#[derive(Debug, Clone, sqlx::FromRow)]
struct AttendanceRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    employee_id: Uuid,
    entry_kind: String,
    recorded_at: DateTime<Utc>,
    local_date: NaiveDate,
    timezone: String,
    source: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    accuracy_meters: Option<f64>,
    note: Option<String>,
    reverses_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl AttendanceRow {
    fn into_dto(self) -> AttendanceDto {
        AttendanceDto {
            id: self.public_id,
            employee_id: PublicId::new(IdKind::Employee, self.employee_id).as_str(),
            entry_kind: self.entry_kind,
            recorded_at: self.recorded_at.to_rfc3339(),
            local_date: self.local_date.to_string(),
            timezone: self.timezone,
            source: self.source,
            latitude: self.latitude,
            longitude: self.longitude,
            accuracy_meters: self.accuracy_meters,
            note: self.note,
            reverses_id: self
                .reverses_id
                .map(|u| PublicId::new(IdKind::AttendanceRecord, u).as_str()),
            created_at: self.created_at.to_rfc3339(),
        }
    }
}

/// GET /api/v1/people/attendance
#[utoipa::path(get, path = "/api/v1/people/attendance", tag = "people-attendance",
    responses((status = 200, body = AttendanceListResponse)))]
pub async fn list_attendance(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<AttendanceListQuery>,
) -> Result<Json<AttendanceListResponse>, AppError> {
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
    let emp_filter = match q.employee_id.as_deref() {
        None | Some("") => None,
        Some(s) => Some(parse_public_id(IdKind::Employee, s, &request_id)?),
    };
    let from = parse_opt_date(q.from.as_deref(), &request_id)?;
    let to = parse_opt_date(q.to.as_deref(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perms::hr_attendance_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*)::bigint FROM people_attendance WHERE org_id = ");
    count_qb.push_bind(org_id);
    push_owner_predicate(
        &mut count_qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    if let Some(eid) = emp_filter {
        count_qb.push(" AND employee_id = ");
        count_qb.push_bind(eid);
    }
    if let Some(f) = from {
        count_qb.push(" AND local_date >= ");
        count_qb.push_bind(f);
    }
    if let Some(t) = to {
        count_qb.push(" AND local_date <= ");
        count_qb.push_bind(t);
    }
    let total: (i64,) = count_qb
        .build_query_as()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        r#"
        SELECT id, public_id, employee_id, entry_kind, recorded_at, local_date, timezone,
               source, latitude, longitude, accuracy_meters, note, reverses_id, created_at
        FROM people_attendance WHERE org_id =
        "#,
    );
    qb.push_bind(org_id);
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    if let Some(eid) = emp_filter {
        qb.push(" AND employee_id = ");
        qb.push_bind(eid);
    }
    if let Some(f) = from {
        qb.push(" AND local_date >= ");
        qb.push_bind(f);
    }
    if let Some(t) = to {
        qb.push(" AND local_date <= ");
        qb.push_bind(t);
    }
    qb.push(" ORDER BY recorded_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);
    let rows: Vec<AttendanceRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(AttendanceListResponse {
        items: rows.into_iter().map(AttendanceRow::into_dto).collect(),
        total: total.0,
    }))
}

/// GET /api/v1/people/me/attendance
#[utoipa::path(get, path = "/api/v1/people/me/attendance", tag = "people-attendance",
    responses((status = 200, body = AttendanceListResponse)))]
pub async fn list_my_attendance(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<AttendanceListQuery>,
) -> Result<Json<AttendanceListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let _ = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let me = fetch_employee_by_user(&mut tx, org_id, auth.ctx.actor.user_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee profile"))?;
    tx.commit().await.map_err(internal(&request_id))?;

    let mut q = q;
    q.employee_id = Some(me.public_id);
    list_attendance(State(state), auth, Query(q)).await
}

/// POST /api/v1/people/attendance
#[utoipa::path(post, path = "/api/v1/people/attendance", tag = "people-attendance",
    request_body = RecordAttendanceRequest,
    responses((status = 201, body = AttendanceDto)))]
pub async fn record_attendance(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<RecordAttendanceRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    if !VALID_KINDS.contains(&body.entry_kind.as_str()) {
        return Err(validation(
            &request_id,
            format!("entry_kind must be one of {VALID_KINDS:?}"),
        ));
    }
    let source = body.source.as_deref().unwrap_or("manual");
    if !["manual", "geo", "csv_import", "system"].contains(&source) {
        return Err(validation(&request_id, "invalid source"));
    }
    if source == "geo" && (body.latitude.is_none() || body.longitude.is_none()) {
        return Err(validation(
            &request_id,
            "geo source requires latitude and longitude",
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
        if let Some((status, cached)) =
            idempotency::get(&mut *tx, org_id, "attendance.record", &key)
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

    let emp = resolve_employee(
        &mut tx,
        org_id,
        &auth,
        body.employee_id.as_deref(),
        &request_id,
    )
    .await?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_attendance_write(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let reverses_uuid = if let Some(rev) = body.reverses_id.as_deref() {
        let rid = parse_public_id(IdKind::AttendanceRecord, rev, &request_id)?;
        let exists: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM people_attendance WHERE org_id = $1 AND id = $2")
                .bind(org_id)
                .bind(rid)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal(&request_id))?;
        if exists.is_none() {
            return Err(not_found(&request_id, "attendance to reverse"));
        }
        if body.entry_kind != "reversal" && body.entry_kind != "adjustment" {
            return Err(validation(
                &request_id,
                "reverses_id requires entry_kind reversal or adjustment",
            ));
        }
        Some(rid)
    } else {
        None
    };

    let recorded_at = match body.recorded_at.as_deref() {
        None | Some("") => Utc::now(),
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| validation(&request_id, "recorded_at must be RFC3339"))?,
    };
    let tz = body.timezone.as_deref().unwrap_or("UTC").to_string();
    let local_date = recorded_at.date_naive();

    let dto = insert_attendance(
        &mut tx,
        org_id,
        &auth,
        &emp,
        &body.entry_kind,
        recorded_at,
        local_date,
        &tz,
        source,
        body.latitude,
        body.longitude,
        body.accuracy_meters,
        body.note.as_deref(),
        reverses_uuid,
        None,
        &request_id,
    )
    .await?;

    let body_json = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "attendance.record", &key, 201, body_json)
            .await
            .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// POST /api/v1/people/attendance/import
#[utoipa::path(post, path = "/api/v1/people/attendance/import", tag = "people-attendance",
    request_body = AttendanceImportRequest,
    responses((status = 201, body = AttendanceImportResponse)))]
pub async fn import_attendance(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<AttendanceImportRequest>,
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
        perms::hr_attendance_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let batch_key = body
        .batch_key
        .clone()
        .or_else(|| idempotency::header_key(&headers));
    if let Some(ref key) = batch_key {
        if let Some((status, cached)) = idempotency::get(&mut *tx, org_id, "attendance.import", key)
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

    let mut imported = 0i64;
    let mut skipped = 0i64;
    let mut items = Vec::new();
    for (line_no, line) in body.csv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("employee_id") {
            continue;
        }
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        if cols.len() < 3 {
            return Err(validation(
                &request_id,
                format!(
                    "line {}: expected employee_id,entry_kind,recorded_at",
                    line_no + 1
                ),
            ));
        }
        let emp_id = parse_public_id(IdKind::Employee, cols[0], &request_id)?;
        let kind = cols[1];
        if !VALID_KINDS.contains(&kind) {
            return Err(validation(
                &request_id,
                format!("line {}: invalid entry_kind", line_no + 1),
            ));
        }
        let recorded_at = DateTime::parse_from_rfc3339(cols[2])
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| {
                validation(
                    &request_id,
                    format!("line {}: recorded_at must be RFC3339", line_no + 1),
                )
            })?;
        let tz = cols.get(4).copied().unwrap_or("UTC");
        let lat = cols.get(5).and_then(|s| s.parse().ok());
        let lng = cols.get(6).and_then(|s| s.parse().ok());
        let acc = cols.get(7).and_then(|s| s.parse().ok());
        let note = cols.get(8).map(|s| s.to_string());

        let emp = fetch_employee_row(&mut tx, org_id, emp_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "employee"))?;
        enforce_employee_scope(
            &mut tx,
            org_id,
            &auth,
            &membership,
            perms::hr_attendance_write(),
            emp.owner_user_id,
            &request_id,
        )
        .await?;

        let import_key = batch_key
            .as_ref()
            .map(|b| format!("{b}:{}:{}:{}", cols[0], kind, cols[2]));
        if let Some(ref sk) = import_key {
            let exists: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM people_attendance WHERE org_id = $1 AND import_batch_key = $2",
            )
            .bind(org_id)
            .bind(sk)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            if exists.is_some() {
                skipped += 1;
                continue;
            }
        }

        let dto = insert_attendance(
            &mut tx,
            org_id,
            &auth,
            &emp,
            kind,
            recorded_at,
            recorded_at.date_naive(),
            tz,
            "csv_import",
            lat,
            lng,
            acc,
            note.as_deref(),
            None,
            import_key.as_deref(),
            &request_id,
        )
        .await?;
        items.push(dto);
        imported += 1;
    }

    let resp = AttendanceImportResponse {
        imported,
        skipped,
        items,
    };
    let body_json = serde_json::to_value(&resp).unwrap_or_default();
    if let Some(ref key) = batch_key {
        idempotency::put(&mut *tx, org_id, "attendance.import", key, 201, body_json)
            .await
            .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(resp)).into_response())
}

#[allow(clippy::too_many_arguments)]
async fn insert_attendance(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    auth: &AuthCtx,
    emp: &EmployeeRow,
    entry_kind: &str,
    recorded_at: DateTime<Utc>,
    local_date: NaiveDate,
    timezone: &str,
    source: &str,
    latitude: Option<f64>,
    longitude: Option<f64>,
    accuracy_meters: Option<f64>,
    note: Option<&str>,
    reverses_id: Option<Uuid>,
    import_batch_key: Option<&str>,
    request_id: &str,
) -> Result<AttendanceDto, AppError> {
    let public_id = PublicId::generate(IdKind::AttendanceRecord);
    let row: AttendanceRow = sqlx::query_as(
        r#"
        INSERT INTO people_attendance (
            id, org_id, public_id, employee_id, entry_kind, recorded_at, local_date,
            timezone, source, latitude, longitude, accuracy_meters, note,
            reverses_id, import_batch_key, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
        RETURNING id, public_id, employee_id, entry_kind, recorded_at, local_date, timezone,
                  source, latitude, longitude, accuracy_meters, note, reverses_id, created_at
        "#,
    )
    .bind(public_id.uuid())
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(emp.id)
    .bind(entry_kind)
    .bind(recorded_at)
    .bind(local_date)
    .bind(timezone)
    .bind(source)
    .bind(latitude)
    .bind(longitude)
    .bind(accuracy_meters)
    .bind(note)
    .bind(reverses_id)
    .bind(import_batch_key)
    .bind(emp.owner_user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let dto = row.into_dto();
    insert_audit(
        &mut **tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.attendance.record",
        "attendance",
        &dto.id,
        serde_json::json!({
            "employee_id": emp.public_id,
            "entry_kind": entry_kind,
            "source": source,
        }),
    )
    .await
    .map_err(internal(request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "attendance",
        "recorded",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "employee_id": emp.public_id,
            "entry_kind": entry_kind,
            "local_date": local_date.to_string(),
        }),
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(dto)
}

async fn resolve_employee(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    auth: &AuthCtx,
    employee_id: Option<&str>,
    request_id: &str,
) -> Result<EmployeeRow, AppError> {
    match employee_id {
        None | Some("") => fetch_employee_by_user(tx, org_id, auth.ctx.actor.user_id)
            .await
            .map_err(internal(request_id))?
            .ok_or_else(|| not_found(request_id, "employee profile")),
        Some(s) => {
            let eid = parse_public_id(IdKind::Employee, s, request_id)?;
            fetch_employee_row(tx, org_id, eid)
                .await
                .map_err(internal(request_id))?
                .ok_or_else(|| not_found(request_id, "employee"))
        }
    }
}

fn parse_opt_date(raw: Option<&str>, request_id: &str) -> Result<Option<NaiveDate>, AppError> {
    match raw {
        None | Some("") => Ok(None),
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| validation(request_id, format!("invalid date: {s}"))),
    }
}
