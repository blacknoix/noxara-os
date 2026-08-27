//! Leave types, requests, balances, calendar, reports, and year-end carry-forward.
//!
//! Leave requests route through the Operations ApprovalProcess (`subject_type=leave_request`).
//! Balances are always derived from the append-only leave ledger.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
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
use super::{conflict, internal, normalize_paging, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::leave_balance::{
    balance_as_of, carry_forward_credit, format_days, leave_units_milli, AccrualPolicy, LedgerEntry,
};
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    AbsenceReportQuery, AbsenceReportResponse, AbsenceReportRowDto, AccrueLeaveRequest,
    CarryForwardRequest, CarryForwardResponse, CreateLeaveRequestRequest, CreateLeaveTypeRequest,
    DecideLeaveRequest, LeaveBalanceDto, LeaveBalanceListResponse, LeaveBalanceQuery,
    LeaveCalendarEntryDto, LeaveCalendarQuery, LeaveCalendarResponse, LeaveLedgerEntryDto,
    LeaveRequestDto, LeaveRequestListQuery, LeaveRequestListResponse, LeaveTypeDto,
    LeaveTypeListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/people/leave-types",
            get(list_leave_types).post(create_leave_type),
        )
        .route("/api/v1/people/leave-types/{id}", get(get_leave_type))
        .route(
            "/api/v1/people/leave-requests",
            get(list_leave_requests).post(create_leave_request),
        )
        .route("/api/v1/people/leave-requests/{id}", get(get_leave_request))
        .route(
            "/api/v1/people/leave-requests/{id}/submit",
            post(submit_leave_request),
        )
        .route(
            "/api/v1/people/leave-requests/{id}/cancel",
            post(cancel_leave_request),
        )
        .route(
            "/api/v1/people/leave-requests/{id}/decide",
            post(decide_leave_request),
        )
        .route("/api/v1/people/leave/balances", get(list_balances))
        .route("/api/v1/people/leave/calendar", get(team_calendar))
        .route("/api/v1/people/leave/reports/absences", get(absence_report))
        .route(
            "/api/v1/people/leave/carry-forward",
            post(run_carry_forward),
        )
        .route("/api/v1/people/leave/accrue", post(accrue_leave))
        .route("/api/v1/people/me/leave", get(list_my_leave))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct LeaveTypeRow {
    id: Uuid,
    public_id: String,
    code: String,
    name: String,
    category: String,
    accrual_cadence: String,
    accrual_units_milli: i32,
    carry_forward_cap_milli: Option<i32>,
    expiry_days: Option<i32>,
    allows_half_day: bool,
    requires_approval: bool,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl LeaveTypeRow {
    fn into_dto(self) -> LeaveTypeDto {
        LeaveTypeDto {
            id: self.public_id,
            code: self.code,
            name: self.name,
            category: self.category,
            accrual_cadence: self.accrual_cadence,
            accrual_units_milli: self.accrual_units_milli,
            carry_forward_cap_milli: self.carry_forward_cap_milli,
            expiry_days: self.expiry_days,
            allows_half_day: self.allows_half_day,
            requires_approval: self.requires_approval,
            is_active: self.is_active,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }

    fn policy(&self) -> AccrualPolicy {
        AccrualPolicy {
            accrual_cadence: self.accrual_cadence.clone(),
            accrual_units_milli: self.accrual_units_milli,
            carry_forward_cap_milli: self.carry_forward_cap_milli,
            expiry_days: self.expiry_days,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct LeaveRequestRow {
    id: Uuid,
    public_id: String,
    employee_id: Uuid,
    leave_type_id: Uuid,
    status: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
    start_period: String,
    end_period: String,
    units_milli: i32,
    timezone: String,
    reason: Option<String>,
    approval_id: Option<String>,
    decided_at: Option<DateTime<Utc>>,
    decision_note: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
    owner_user_id: Uuid,
}

impl LeaveRequestRow {
    fn into_dto(self) -> LeaveRequestDto {
        LeaveRequestDto {
            id: self.public_id,
            employee_id: PublicId::new(IdKind::Employee, self.employee_id).as_str(),
            leave_type_id: PublicId::new(IdKind::LeaveType, self.leave_type_id).as_str(),
            status: self.status,
            start_date: self.start_date.to_string(),
            end_date: self.end_date.to_string(),
            start_period: self.start_period,
            end_period: self.end_period,
            units_milli: self.units_milli,
            units_days: format_days(self.units_milli),
            timezone: self.timezone,
            reason: self.reason,
            approval_id: self.approval_id,
            decided_at: self.decided_at.map(|t| t.to_rfc3339()),
            decision_note: self.decision_note,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

const LT_COLS: &str = r#"
    id, public_id, code, name, category, accrual_cadence, accrual_units_milli,
    carry_forward_cap_milli, expiry_days, allows_half_day, requires_approval, is_active,
    created_at, updated_at, version
"#;

const LR_COLS: &str = r#"
    id, public_id, employee_id, leave_type_id, status, start_date, end_date,
    start_period, end_period, units_milli, timezone, reason, approval_id,
    decided_at, decision_note, created_at, updated_at, version, owner_user_id
"#;

/// GET /api/v1/people/leave-types
#[utoipa::path(get, path = "/api/v1/people/leave-types", tag = "people-leave",
    responses((status = 200, body = LeaveTypeListResponse)))]
pub async fn list_leave_types(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<LeaveTypeListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_read(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let rows: Vec<LeaveTypeRow> = sqlx::query_as(&format!(
        "SELECT {LT_COLS} FROM people_leave_type
         WHERE org_id = $1 AND deleted_at IS NULL ORDER BY code"
    ))
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(LeaveTypeListResponse {
        items: rows.into_iter().map(LeaveTypeRow::into_dto).collect(),
    }))
}

/// POST /api/v1/people/leave-types
#[utoipa::path(post, path = "/api/v1/people/leave-types", tag = "people-leave",
    request_body = CreateLeaveTypeRequest,
    responses((status = 201, body = LeaveTypeDto)))]
pub async fn create_leave_type(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateLeaveTypeRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    if body.code.trim().is_empty() || body.name.trim().is_empty() {
        return Err(validation(&request_id, "code and name are required"));
    }
    let category = body.category.as_deref().unwrap_or("annual");
    if !["annual", "sick", "unpaid", "custom"].contains(&category) {
        return Err(validation(&request_id, "invalid category"));
    }
    let cadence = body.accrual_cadence.as_deref().unwrap_or("yearly");
    if !["none", "monthly", "yearly", "on_hire"].contains(&cadence) {
        return Err(validation(&request_id, "invalid accrual_cadence"));
    }
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    // Leave type admin: write + typically manager+ (Member has write for requests
    // but creating types is gated the same; managers own catalogue).
    enforce_any_scope(&membership.principal, perms::hr_leave_write(), &request_id)?;
    // Members may request leave but not invent org leave types.
    let is_member_only = membership.principal.roles.len() == 1
        && membership
            .principal
            .roles
            .iter()
            .any(|r| r.as_str() == "member");
    if is_member_only {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "members cannot create leave types",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) =
            idempotency::get(&mut *tx, org_id, "leave_type.create", &key)
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

    let public_id = PublicId::generate(IdKind::LeaveType);
    let row: LeaveTypeRow = sqlx::query_as(&format!(
        r#"
        INSERT INTO people_leave_type (
            id, org_id, public_id, code, name, category, accrual_cadence,
            accrual_units_milli, carry_forward_cap_milli, expiry_days,
            allows_half_day, requires_approval, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
        RETURNING {LT_COLS}
        "#
    ))
    .bind(public_id.uuid())
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.code.trim().to_ascii_uppercase())
    .bind(body.name.trim())
    .bind(category)
    .bind(cadence)
    .bind(body.accrual_units_milli.unwrap_or(0))
    .bind(body.carry_forward_cap_milli)
    .bind(body.expiry_days)
    .bind(body.allows_half_day.unwrap_or(true))
    .bind(body.requires_approval.unwrap_or(true))
    .bind(auth.ctx.actor.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db) = &e {
            if db.constraint().is_some_and(|c| c.contains("code")) {
                return conflict(&request_id, "leave type code already exists");
            }
        }
        internal(&request_id)(e)
    })?;

    let dto = row.into_dto();
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.leave_type.create",
        "leave_type",
        &dto.id,
        serde_json::json!({ "code": dto.code }),
    )
    .await
    .map_err(internal(&request_id))?;

    let body_json = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "leave_type.create", &key, 201, body_json)
            .await
            .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// GET /api/v1/people/leave-types/{id}
#[utoipa::path(get, path = "/api/v1/people/leave-types/{id}", tag = "people-leave",
    params(("id" = String, Path)),
    responses((status = 200, body = LeaveTypeDto)))]
pub async fn get_leave_type(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<LeaveTypeDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let tid = parse_public_id(IdKind::LeaveType, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_read(), &request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row: Option<LeaveTypeRow> = sqlx::query_as(&format!(
        "SELECT {LT_COLS} FROM people_leave_type
         WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(tid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(
        row.ok_or_else(|| not_found(&request_id, "leave type"))?
            .into_dto(),
    ))
}

/// GET /api/v1/people/leave-requests
#[utoipa::path(get, path = "/api/v1/people/leave-requests", tag = "people-leave",
    responses((status = 200, body = LeaveRequestListResponse)))]
pub async fn list_leave_requests(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<LeaveRequestListQuery>,
) -> Result<Json<LeaveRequestListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_read(), &request_id)?;
    let (limit, offset) = normalize_paging(q.limit, q.offset);
    let emp_filter = match q.employee_id.as_deref() {
        None | Some("") => None,
        Some(s) => Some(parse_public_id(IdKind::Employee, s, &request_id)?),
    };
    let from = parse_opt_date(q.from.as_deref(), &request_id)?;
    let to = parse_opt_date(q.to.as_deref(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perms::hr_leave_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*)::bigint FROM people_leave_request WHERE org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND deleted_at IS NULL");
    push_owner_predicate(
        &mut count_qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    apply_leave_filters(&mut count_qb, emp_filter, q.status.as_deref(), from, to);
    let total: (i64,) = count_qb
        .build_query_as()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {LR_COLS} FROM people_leave_request WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    apply_leave_filters(&mut qb, emp_filter, q.status.as_deref(), from, to);
    qb.push(" ORDER BY start_date DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);
    let rows: Vec<LeaveRequestRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(LeaveRequestListResponse {
        items: rows.into_iter().map(LeaveRequestRow::into_dto).collect(),
        total: total.0,
    }))
}

/// GET /api/v1/people/me/leave
#[utoipa::path(get, path = "/api/v1/people/me/leave", tag = "people-leave",
    responses((status = 200, body = LeaveRequestListResponse)))]
pub async fn list_my_leave(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<LeaveRequestListQuery>,
) -> Result<Json<LeaveRequestListResponse>, AppError> {
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
    list_leave_requests(State(state), auth, Query(q)).await
}

/// POST /api/v1/people/leave-requests
#[utoipa::path(post, path = "/api/v1/people/leave-requests", tag = "people-leave",
    request_body = CreateLeaveRequestRequest,
    responses((status = 201, body = LeaveRequestDto)))]
pub async fn create_leave_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateLeaveRequestRequest>,
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
    enforce_any_scope(&membership.principal, perms::hr_leave_write(), &request_id)?;

    let start = parse_date(&body.start_date, &request_id)?;
    let end = parse_date(&body.end_date, &request_id)?;
    if end < start {
        return Err(validation(&request_id, "end_date must be >= start_date"));
    }
    let start_period = body.start_period.as_deref().unwrap_or("full");
    let end_period = body.end_period.as_deref().unwrap_or("full");
    for p in [start_period, end_period] {
        if !["full", "am", "pm"].contains(&p) {
            return Err(validation(&request_id, "period must be full|am|pm"));
        }
    }
    let leave_type_id = parse_public_id(IdKind::LeaveType, &body.leave_type_id, &request_id)?;
    let tz = body.timezone.as_deref().unwrap_or("UTC").to_string();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) =
            idempotency::get(&mut *tx, org_id, "leave_request.create", &key)
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
    // Members may only request for themselves.
    if emp.user_id != Some(auth.ctx.actor.user_id) {
        enforce_employee_scope(
            &mut tx,
            org_id,
            &auth,
            &membership,
            perms::hr_leave_write(),
            emp.owner_user_id,
            &request_id,
        )
        .await?;
        let can_manage = membership
            .principal
            .roles
            .iter()
            .any(|r| matches!(r.as_str(), "owner" | "admin" | "manager"));
        if !can_manage {
            return Err(AppError::new(
                ErrorCode::Forbidden,
                request_id,
                "cannot request leave for another employee",
            ));
        }
    }

    let lt: LeaveTypeRow = sqlx::query_as(&format!(
        "SELECT {LT_COLS} FROM people_leave_type
         WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL AND is_active = true"
    ))
    .bind(org_id)
    .bind(leave_type_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .ok_or_else(|| not_found(&request_id, "leave type"))?;

    if !lt.allows_half_day && (start_period != "full" || end_period != "full") {
        return Err(validation(
            &request_id,
            "leave type does not allow half-day",
        ));
    }

    let (full_holidays, half_holidays) =
        load_holidays(&mut tx, org_id, start, end, &request_id).await?;
    let units = leave_units_milli(
        start,
        end,
        start_period,
        end_period,
        true,
        &full_holidays,
        &half_holidays,
    );
    if units <= 0 {
        return Err(validation(&request_id, "request covers no working days"));
    }

    // Unpaid leave may go negative; others need sufficient balance when submitting.
    let public_id = PublicId::generate(IdKind::LeaveRequest);
    let row: LeaveRequestRow = sqlx::query_as(&format!(
        r#"
        INSERT INTO people_leave_request (
            id, org_id, public_id, employee_id, leave_type_id, status,
            start_date, end_date, start_period, end_period, units_milli,
            timezone, reason, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,'draft',$6,$7,$8,$9,$10,$11,$12,$13)
        RETURNING {LR_COLS}
        "#
    ))
    .bind(public_id.uuid())
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(emp.id)
    .bind(leave_type_id)
    .bind(start)
    .bind(end)
    .bind(start_period)
    .bind(end_period)
    .bind(units)
    .bind(&tz)
    .bind(body.reason.as_deref())
    .bind(emp.owner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.leave_request.create",
        "leave_request",
        &row.public_id,
        serde_json::json!({ "units_milli": units }),
    )
    .await
    .map_err(internal(&request_id))?;

    let mut dto = row.into_dto();
    let submit = body.submit.unwrap_or(false);
    if submit {
        dto = do_submit(
            &mut tx,
            &state,
            &auth,
            org_id,
            &dto.id,
            &lt,
            &emp,
            &request_id,
        )
        .await?;
    }

    let body_json = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(
            &mut *tx,
            org_id,
            "leave_request.create",
            &key,
            201,
            body_json,
        )
        .await
        .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// GET /api/v1/people/leave-requests/{id}
#[utoipa::path(get, path = "/api/v1/people/leave-requests/{id}", tag = "people-leave",
    params(("id" = String, Path)),
    responses((status = 200, body = LeaveRequestDto)))]
pub async fn get_leave_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<LeaveRequestDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let rid = parse_public_id(IdKind::LeaveRequest, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_read(), &request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_leave_request(&mut tx, org_id, rid, &request_id).await?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_leave_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// POST /api/v1/people/leave-requests/{id}/submit
#[utoipa::path(post, path = "/api/v1/people/leave-requests/{id}/submit", tag = "people-leave",
    params(("id" = String, Path)),
    responses((status = 200, body = LeaveRequestDto)))]
pub async fn submit_leave_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let rid = parse_public_id(IdKind::LeaveRequest, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_write(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) =
            idempotency::get(&mut *tx, org_id, "leave_request.submit", &key)
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

    let row = fetch_leave_request(&mut tx, org_id, rid, &request_id).await?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_leave_write(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    if row.status != "draft" {
        return Err(conflict(
            &request_id,
            format!("leave request status {} is not draft", row.status),
        ));
    }
    let emp = fetch_employee_row(&mut tx, org_id, row.employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    let lt: LeaveTypeRow = sqlx::query_as(&format!(
        "SELECT {LT_COLS} FROM people_leave_type WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(row.leave_type_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = do_submit(
        &mut tx,
        &state,
        &auth,
        org_id,
        &row.public_id,
        &lt,
        &emp,
        &request_id,
    )
    .await?;

    let body_json = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(
            &mut *tx,
            org_id,
            "leave_request.submit",
            &key,
            200,
            body_json,
        )
        .await
        .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto).into_response())
}

/// POST /api/v1/people/leave-requests/{id}/cancel
#[utoipa::path(post, path = "/api/v1/people/leave-requests/{id}/cancel", tag = "people-leave",
    params(("id" = String, Path)),
    responses((status = 200, body = LeaveRequestDto)))]
pub async fn cancel_leave_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<LeaveRequestDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let rid = parse_public_id(IdKind::LeaveRequest, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_write(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_leave_request(&mut tx, org_id, rid, &request_id).await?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_leave_write(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    if !["draft", "pending_approval", "approved"].contains(&row.status.as_str()) {
        return Err(conflict(
            &request_id,
            format!("cannot cancel leave in status {}", row.status),
        ));
    }
    let was_approved = row.status == "approved";
    let updated: LeaveRequestRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_leave_request SET
            status = 'cancelled', version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {LR_COLS}
        "#
    ))
    .bind(org_id)
    .bind(rid)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if was_approved {
        // Credit back the debit.
        insert_ledger_entry(
            &mut tx,
            org_id,
            &auth,
            updated.employee_id,
            updated.leave_type_id,
            "credit",
            updated.units_milli,
            Utc::now().date_naive(),
            None,
            Some(updated.id),
            Some("cancellation credit"),
            Some(&format!("cancel:{}", updated.public_id)),
            updated.owner_user_id,
            &request_id,
        )
        .await?;
        // Clear on_leave if no other approved overlapping leave.
        maybe_clear_on_leave(&mut tx, org_id, updated.employee_id, &request_id).await?;
    }

    let dto = updated.into_dto();
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.leave_request.cancel",
        "leave_request",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "leave",
        "cancelled",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "employee_id": dto.employee_id }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/people/leave-requests/{id}/decide — approval engine callback.
#[utoipa::path(post, path = "/api/v1/people/leave-requests/{id}/decide", tag = "people-leave",
    request_body = DecideLeaveRequest,
    params(("id" = String, Path)),
    responses((status = 200, body = LeaveRequestDto)))]
pub async fn decide_leave_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<DecideLeaveRequest>,
) -> Result<Json<LeaveRequestDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let rid = parse_public_id(IdKind::LeaveRequest, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_leave_approve(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_leave_request(&mut tx, org_id, rid, &request_id).await?;

    // Policy: cannot approve own leave.
    let emp = fetch_employee_row(&mut tx, org_id, row.employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    if emp.user_id == Some(auth.ctx.actor.user_id) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "cannot approve own leave request",
        ));
    }

    if row.status != "pending_approval" {
        // Idempotent: already decided.
        if (body.approve && row.status == "approved") || (!body.approve && row.status == "rejected")
        {
            tx.commit().await.map_err(internal(&request_id))?;
            return Ok(Json(row.into_dto()));
        }
        return Err(conflict(
            &request_id,
            format!(
                "leave request status {} is not pending_approval",
                row.status
            ),
        ));
    }

    let new_status = if body.approve { "approved" } else { "rejected" };
    let updated: LeaveRequestRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_leave_request SET
            status = $3, decided_at = now(), decision_note = $4,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {LR_COLS}
        "#
    ))
    .bind(org_id)
    .bind(rid)
    .bind(new_status)
    .bind(body.note.as_deref())
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if body.approve {
        insert_ledger_entry(
            &mut tx,
            org_id,
            &auth,
            updated.employee_id,
            updated.leave_type_id,
            "debit",
            -updated.units_milli,
            updated.start_date,
            None,
            Some(updated.id),
            Some("leave approved"),
            Some(&format!("debit:{}", updated.public_id)),
            updated.owner_user_id,
            &request_id,
        )
        .await?;
        // Mark employee on_leave when leave is current.
        let today = Utc::now().date_naive();
        if updated.start_date <= today && updated.end_date >= today {
            sqlx::query(
                "UPDATE people_employee SET status = 'on_leave', updated_at = now(), version = version + 1
                 WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'active'",
            )
            .bind(org_id)
            .bind(updated.employee_id)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        }
    }

    let dto = updated.into_dto();
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        if body.approve {
            "hr.leave_request.approve"
        } else {
            "hr.leave_request.reject"
        },
        "leave_request",
        &dto.id,
        serde_json::json!({ "note": body.note }),
    )
    .await
    .map_err(internal(&request_id))?;

    let event_type = if body.approve { "approved" } else { "rejected" };
    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "leave",
        event_type,
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "employee_id": dto.employee_id,
            "status": dto.status,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    // Ledger posted event when approved.
    if body.approve {
        let posted = EventEnvelope::new(
            auth.ctx.org_id,
            Context::People,
            "leave",
            "ledger_posted",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "leave_request_id": dto.id,
                "employee_id": dto.employee_id,
                "units_milli": -dto.units_milli,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &posted)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// GET /api/v1/people/leave/balances
#[utoipa::path(get, path = "/api/v1/people/leave/balances", tag = "people-leave",
    responses((status = 200, body = LeaveBalanceListResponse)))]
pub async fn list_balances(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<LeaveBalanceQuery>,
) -> Result<Json<LeaveBalanceListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_read(), &request_id)?;
    let as_of = match q.as_of.as_deref() {
        None | Some("") => Utc::now().date_naive(),
        Some(s) => parse_date(s, &request_id)?,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let emp = match q.employee_id.as_deref() {
        None | Some("") => fetch_employee_by_user(&mut tx, org_id, auth.ctx.actor.user_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "employee profile"))?,
        Some(s) => {
            let eid = parse_public_id(IdKind::Employee, s, &request_id)?;
            let emp = fetch_employee_row(&mut tx, org_id, eid)
                .await
                .map_err(internal(&request_id))?
                .ok_or_else(|| not_found(&request_id, "employee"))?;
            enforce_employee_scope(
                &mut tx,
                org_id,
                &auth,
                &membership,
                perms::hr_leave_read(),
                emp.owner_user_id,
                &request_id,
            )
            .await?;
            emp
        }
    };

    let types: Vec<LeaveTypeRow> = sqlx::query_as(&format!(
        "SELECT {LT_COLS} FROM people_leave_type
         WHERE org_id = $1 AND deleted_at IS NULL AND is_active = true ORDER BY code"
    ))
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut items = Vec::new();
    for lt in types {
        let entries = load_ledger_entries(&mut tx, org_id, emp.id, lt.id, &request_id).await?;
        let bal = balance_as_of(&entries, as_of);
        items.push(LeaveBalanceDto {
            employee_id: emp.public_id.clone(),
            leave_type_id: lt.public_id.clone(),
            leave_type_code: lt.code.clone(),
            leave_type_name: lt.name.clone(),
            balance_units_milli: bal,
            balance_days: format_days(bal),
            as_of: as_of.to_string(),
        });
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(LeaveBalanceListResponse { items }))
}

/// GET /api/v1/people/leave/calendar
#[utoipa::path(get, path = "/api/v1/people/leave/calendar", tag = "people-leave",
    responses((status = 200, body = LeaveCalendarResponse)))]
pub async fn team_calendar(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<LeaveCalendarQuery>,
) -> Result<Json<LeaveCalendarResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_read(), &request_id)?;
    let from = parse_opt_date(q.from.as_deref(), &request_id)?
        .unwrap_or_else(|| Utc::now().date_naive().with_day(1).unwrap());
    let to = parse_opt_date(q.to.as_deref(), &request_id)?
        .unwrap_or_else(|| from + chrono::Duration::days(31));
    let scope = scope_for_permission(&membership.principal, &perms::hr_leave_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    #[derive(sqlx::FromRow)]
    struct CalRow {
        public_id: String,
        emp_public: String,
        display_name: String,
        code: String,
        status: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        start_period: String,
        end_period: String,
        units_milli: i32,
    }

    let rows: Vec<CalRow> = sqlx::query_as(
        r#"
        SELECT lr.public_id, e.public_id AS emp_public, e.display_name, lt.code,
               lr.status, lr.start_date, lr.end_date, lr.start_period, lr.end_period, lr.units_milli
        FROM people_leave_request lr
        JOIN people_employee e ON e.id = lr.employee_id AND e.org_id = lr.org_id
        JOIN people_leave_type lt ON lt.id = lr.leave_type_id AND lt.org_id = lr.org_id
        WHERE lr.org_id = $1 AND lr.deleted_at IS NULL
          AND lr.status IN ('approved','pending_approval')
          AND lr.start_date <= $2 AND lr.end_date >= $3
          AND (
            $4::text = 'organization'
            OR lr.owner_user_id = $5
            OR ($4::text = 'team' AND $6::uuid IS NOT NULL AND lr.owner_user_id IN (
                  SELECT user_id FROM membership
                  WHERE org_id = $1 AND team_id = $6 AND revoked_at IS NULL))
            OR ($4::text = 'department' AND $7::uuid IS NOT NULL AND lr.owner_user_id IN (
                  SELECT user_id FROM membership
                  WHERE org_id = $1 AND department_id = $7 AND revoked_at IS NULL))
          )
        ORDER BY lr.start_date, e.display_name
        "#,
    )
    .bind(org_id)
    .bind(to)
    .bind(from)
    .bind(match scope {
        companyos_authz::Scope::Organization => "organization",
        companyos_authz::Scope::Team => "team",
        companyos_authz::Scope::Department => "department",
        companyos_authz::Scope::Own => "own",
    })
    .bind(auth.ctx.actor.user_id)
    .bind(membership.team_id)
    .bind(membership.department_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(LeaveCalendarResponse {
        items: rows
            .into_iter()
            .map(|r| LeaveCalendarEntryDto {
                leave_request_id: r.public_id,
                employee_id: r.emp_public,
                employee_display_name: r.display_name,
                leave_type_code: r.code,
                status: r.status,
                start_date: r.start_date.to_string(),
                end_date: r.end_date.to_string(),
                start_period: r.start_period,
                end_period: r.end_period,
                units_milli: r.units_milli,
            })
            .collect(),
    }))
}

/// GET /api/v1/people/leave/reports/absences
#[utoipa::path(get, path = "/api/v1/people/leave/reports/absences", tag = "people-leave",
    responses((status = 200, body = AbsenceReportResponse)))]
pub async fn absence_report(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<AbsenceReportQuery>,
) -> Result<Json<AbsenceReportResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_read(), &request_id)?;
    let from = parse_opt_date(q.from.as_deref(), &request_id)?
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(Utc::now().year(), 1, 1).unwrap());
    let to =
        parse_opt_date(q.to.as_deref(), &request_id)?.unwrap_or_else(|| Utc::now().date_naive());
    let scope = scope_for_permission(&membership.principal, &perms::hr_leave_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    #[derive(sqlx::FromRow)]
    struct Row {
        emp_public: String,
        display_name: String,
        code: String,
        units_milli: i64,
        request_count: i64,
    }

    let rows: Vec<Row> = sqlx::query_as(
        r#"
        SELECT e.public_id AS emp_public, e.display_name, lt.code,
               COALESCE(SUM(lr.units_milli),0)::bigint AS units_milli,
               COUNT(*)::bigint AS request_count
        FROM people_leave_request lr
        JOIN people_employee e ON e.id = lr.employee_id AND e.org_id = lr.org_id
        JOIN people_leave_type lt ON lt.id = lr.leave_type_id AND lt.org_id = lr.org_id
        WHERE lr.org_id = $1 AND lr.deleted_at IS NULL AND lr.status = 'approved'
          AND lr.start_date <= $2 AND lr.end_date >= $3
          AND (
            $4::text = 'organization'
            OR lr.owner_user_id = $5
            OR ($4::text = 'team' AND $6::uuid IS NOT NULL AND lr.owner_user_id IN (
                  SELECT user_id FROM membership
                  WHERE org_id = $1 AND team_id = $6 AND revoked_at IS NULL))
            OR ($4::text = 'department' AND $7::uuid IS NOT NULL AND lr.owner_user_id IN (
                  SELECT user_id FROM membership
                  WHERE org_id = $1 AND department_id = $7 AND revoked_at IS NULL))
          )
        GROUP BY e.public_id, e.display_name, lt.code
        ORDER BY e.display_name, lt.code
        "#,
    )
    .bind(org_id)
    .bind(to)
    .bind(from)
    .bind(match scope {
        companyos_authz::Scope::Organization => "organization",
        companyos_authz::Scope::Team => "team",
        companyos_authz::Scope::Department => "department",
        companyos_authz::Scope::Own => "own",
    })
    .bind(auth.ctx.actor.user_id)
    .bind(membership.team_id)
    .bind(membership.department_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(AbsenceReportResponse {
        items: rows
            .into_iter()
            .map(|r| {
                let units = r.units_milli as i32;
                AbsenceReportRowDto {
                    employee_id: r.emp_public,
                    employee_display_name: r.display_name,
                    leave_type_code: r.code,
                    units_milli: units,
                    units_days: format_days(units),
                    request_count: r.request_count,
                }
            })
            .collect(),
        from: from.to_string(),
        to: to.to_string(),
    }))
}

/// POST /api/v1/people/leave/carry-forward — idempotent Temporal-style workflow.
#[utoipa::path(post, path = "/api/v1/people/leave/carry-forward", tag = "people-leave",
    request_body = CarryForwardRequest,
    responses((status = 200, body = CarryForwardResponse)))]
pub async fn run_carry_forward(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CarryForwardRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    if !(2000..=2100).contains(&body.year) {
        return Err(validation(&request_id, "year out of range"));
    }
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(&membership.principal, perms::hr_leave_write(), &request_id)?;
    let is_member_only = membership.principal.roles.len() == 1
        && membership
            .principal
            .roles
            .iter()
            .any(|r| r.as_str() == "member");
    if is_member_only {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "members cannot run year-end carry-forward",
        ));
    }

    let org_public = auth.ctx.org_id.to_public().as_str();
    let workflow_id = format!("{org_public}:LeaveCarryForward:{}", body.year);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) =
            idempotency::get(&mut *tx, org_id, "leave.carry_forward", &key)
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

    // Idempotent: existing completed run returns prior result.
    let existing: Option<(String, i32, String)> = sqlx::query_as(
        r#"
        SELECT workflow_id, entries_posted, status
        FROM people_leave_carry_forward_run
        WHERE org_id = $1 AND year = $2
        "#,
    )
    .bind(org_id)
    .bind(body.year)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Some((wf, posted, status)) = existing {
        let resp = CarryForwardResponse {
            workflow_id: wf,
            year: body.year,
            status,
            entries_posted: posted,
            idempotent_replay: true,
        };
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(resp).into_response());
    }

    let year_end = NaiveDate::from_ymd_opt(body.year, 12, 31)
        .ok_or_else(|| validation(&request_id, "bad year"))?;
    let next_year_start = NaiveDate::from_ymd_opt(body.year + 1, 1, 1)
        .ok_or_else(|| validation(&request_id, "bad year"))?;

    let types: Vec<LeaveTypeRow> = sqlx::query_as(&format!(
        "SELECT {LT_COLS} FROM people_leave_type
         WHERE org_id = $1 AND deleted_at IS NULL AND is_active = true"
    ))
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let employees: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, owner_user_id FROM people_employee
         WHERE org_id = $1 AND deleted_at IS NULL AND status IN ('active','on_leave','onboarding')",
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut entries_posted = 0i32;
    for (emp_id, owner) in &employees {
        for lt in &types {
            if lt.category == "unpaid" || lt.accrual_cadence == "none" {
                continue;
            }
            let entries = load_ledger_entries(&mut tx, org_id, *emp_id, lt.id, &request_id).await?;
            let bal = balance_as_of(&entries, year_end);
            let policy = lt.policy();
            // Zero prior year then post capped carry-forward.
            let source_zero = format!("cf-zero:{}:{}", body.year, lt.public_id);
            let source_cf = format!("cf:{}:{}", body.year, lt.public_id);

            if bal != 0 {
                insert_ledger_entry(
                    &mut tx,
                    org_id,
                    &auth,
                    *emp_id,
                    lt.id,
                    "expiry",
                    -bal,
                    next_year_start,
                    None,
                    None,
                    Some("year-end zero"),
                    Some(&source_zero),
                    *owner,
                    &request_id,
                )
                .await?;
                entries_posted += 1;
            }
            if let Some((cf, exp)) = carry_forward_credit(bal.max(0), &policy, year_end) {
                insert_ledger_entry(
                    &mut tx,
                    org_id,
                    &auth,
                    *emp_id,
                    lt.id,
                    "carry_forward",
                    cf,
                    next_year_start,
                    exp,
                    None,
                    Some("year-end carry-forward"),
                    Some(&source_cf),
                    *owner,
                    &request_id,
                )
                .await?;
                entries_posted += 1;
            }
        }
    }

    let run_id = companyos_ids::new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO people_leave_carry_forward_run (
            id, org_id, year, workflow_id, status, entries_posted, completed_at
        ) VALUES ($1,$2,$3,$4,'completed',$5,now())
        "#,
    )
    .bind(run_id)
    .bind(org_id)
    .bind(body.year)
    .bind(&workflow_id)
    .bind(entries_posted)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.leave.carry_forward",
        "leave_carry_forward",
        &workflow_id,
        serde_json::json!({ "year": body.year, "entries_posted": entries_posted }),
    )
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "leave",
        "carry_forward",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "workflow_id": workflow_id,
            "year": body.year,
            "entries_posted": entries_posted,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let resp = CarryForwardResponse {
        workflow_id,
        year: body.year,
        status: "completed".into(),
        entries_posted,
        idempotent_replay: false,
    };
    let body_json = serde_json::to_value(&resp).unwrap_or_default();
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(
            &mut *tx,
            org_id,
            "leave.carry_forward",
            &key,
            200,
            body_json,
        )
        .await
        .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(resp).into_response())
}

/// POST /api/v1/people/leave/accrue — post an accrual ledger entry (admin).
#[utoipa::path(post, path = "/api/v1/people/leave/accrue", tag = "people-leave",
    request_body = AccrueLeaveRequest,
    responses((status = 201, body = LeaveLedgerEntryDto)))]
pub async fn accrue_leave(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<AccrueLeaveRequest>,
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
    enforce_any_scope(&membership.principal, perms::hr_leave_write(), &request_id)?;
    let is_member_only = membership.principal.roles.len() == 1
        && membership
            .principal
            .roles
            .iter()
            .any(|r| r.as_str() == "member");
    if is_member_only {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "members cannot post accruals",
        ));
    }

    let emp_id = parse_public_id(IdKind::Employee, &body.employee_id, &request_id)?;
    let lt_id = parse_public_id(IdKind::LeaveType, &body.leave_type_id, &request_id)?;
    let effective = match body.effective_date.as_deref() {
        None | Some("") => Utc::now().date_naive(),
        Some(s) => parse_date(s, &request_id)?,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) = idempotency::get(&mut *tx, org_id, "leave.accrue", &key)
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

    let emp = fetch_employee_row(&mut tx, org_id, emp_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    let lt: LeaveTypeRow = sqlx::query_as(&format!(
        "SELECT {LT_COLS} FROM people_leave_type WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(lt_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .ok_or_else(|| not_found(&request_id, "leave type"))?;

    let units = body.units_milli.unwrap_or(lt.accrual_units_milli);
    if units <= 0 {
        return Err(validation(&request_id, "units_milli must be positive"));
    }

    let dto = insert_ledger_entry(
        &mut tx,
        org_id,
        &auth,
        emp.id,
        lt.id,
        "accrual",
        units,
        effective,
        None,
        None,
        body.note.as_deref(),
        None,
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let body_json = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "leave.accrue", &key, 201, body_json)
            .await
            .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn apply_leave_filters(
    qb: &mut QueryBuilder<'_, Postgres>,
    emp_filter: Option<Uuid>,
    status: Option<&str>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) {
    if let Some(eid) = emp_filter {
        qb.push(" AND employee_id = ");
        qb.push_bind(eid);
    }
    if let Some(s) = status {
        qb.push(" AND status = ");
        qb.push_bind(s.to_string());
    }
    if let Some(f) = from {
        qb.push(" AND end_date >= ");
        qb.push_bind(f);
    }
    if let Some(t) = to {
        qb.push(" AND start_date <= ");
        qb.push_bind(t);
    }
}

async fn fetch_leave_request(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    id: Uuid,
    request_id: &str,
) -> Result<LeaveRequestRow, AppError> {
    sqlx::query_as(&format!(
        "SELECT {LR_COLS} FROM people_leave_request
         WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "leave request"))
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

async fn load_holidays(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
    request_id: &str,
) -> Result<(Vec<NaiveDate>, Vec<NaiveDate>), AppError> {
    #[derive(sqlx::FromRow)]
    struct H {
        holiday_date: NaiveDate,
        is_half_day: bool,
    }
    let rows: Vec<H> = sqlx::query_as(
        r#"
        SELECT holiday_date, is_half_day FROM people_holiday
        WHERE org_id = $1 AND deleted_at IS NULL
          AND holiday_date >= $2 AND holiday_date <= $3
        "#,
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    let mut full = Vec::new();
    let mut half = Vec::new();
    for r in rows {
        if r.is_half_day {
            half.push(r.holiday_date);
        } else {
            full.push(r.holiday_date);
        }
    }
    Ok((full, half))
}

async fn load_ledger_entries(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    leave_type_id: Uuid,
    request_id: &str,
) -> Result<Vec<LedgerEntry>, AppError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        entry_kind: String,
        units_milli: i32,
        effective_date: NaiveDate,
        expires_on: Option<NaiveDate>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        r#"
        SELECT entry_kind, units_milli, effective_date, expires_on
        FROM people_leave_ledger
        WHERE org_id = $1 AND employee_id = $2 AND leave_type_id = $3
        ORDER BY effective_date, created_at
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .bind(leave_type_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(rows
        .into_iter()
        .map(|r| LedgerEntry {
            entry_kind: r.entry_kind,
            units_milli: r.units_milli,
            effective_date: r.effective_date,
            expires_on: r.expires_on,
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
async fn insert_ledger_entry(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    auth: &AuthCtx,
    employee_id: Uuid,
    leave_type_id: Uuid,
    entry_kind: &str,
    units_milli: i32,
    effective_date: NaiveDate,
    expires_on: Option<NaiveDate>,
    leave_request_id: Option<Uuid>,
    note: Option<&str>,
    source_key: Option<&str>,
    owner_user_id: Uuid,
    request_id: &str,
) -> Result<LeaveLedgerEntryDto, AppError> {
    // Idempotent when source_key set.
    if let Some(sk) = source_key {
        #[derive(sqlx::FromRow)]
        struct ExistingLedger {
            public_id: String,
            units_milli: i32,
            effective_date: NaiveDate,
            expires_on: Option<NaiveDate>,
            leave_request_id: Option<Uuid>,
            note: Option<String>,
            created_at: DateTime<Utc>,
        }
        let existing: Option<ExistingLedger> = sqlx::query_as(
            r#"
                SELECT public_id, units_milli, effective_date, expires_on, leave_request_id, note, created_at
                FROM people_leave_ledger
                WHERE org_id = $1 AND employee_id = $2 AND leave_type_id = $3 AND source_key = $4
                "#,
        )
        .bind(org_id)
        .bind(employee_id)
        .bind(leave_type_id)
        .bind(sk)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal(request_id))?;
        if let Some(row) = existing {
            return Ok(LeaveLedgerEntryDto {
                id: row.public_id,
                employee_id: PublicId::new(IdKind::Employee, employee_id).as_str(),
                leave_type_id: PublicId::new(IdKind::LeaveType, leave_type_id).as_str(),
                entry_kind: entry_kind.to_string(),
                units_milli: row.units_milli,
                effective_date: row.effective_date.to_string(),
                expires_on: row.expires_on.map(|d| d.to_string()),
                leave_request_id: row
                    .leave_request_id
                    .map(|u| PublicId::new(IdKind::LeaveRequest, u).as_str()),
                note: row.note,
                created_at: row.created_at.to_rfc3339(),
            });
        }
    }

    let public_id = PublicId::generate(IdKind::LeaveLedgerEntry);
    #[derive(sqlx::FromRow)]
    struct Row {
        public_id: String,
        units_milli: i32,
        effective_date: NaiveDate,
        expires_on: Option<NaiveDate>,
        leave_request_id: Option<Uuid>,
        note: Option<String>,
        created_at: DateTime<Utc>,
        entry_kind: String,
    }
    let row: Row = sqlx::query_as(
        r#"
        INSERT INTO people_leave_ledger (
            id, org_id, public_id, employee_id, leave_type_id, entry_kind,
            units_milli, effective_date, expires_on, leave_request_id, note,
            source_key, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
        RETURNING public_id, units_milli, effective_date, expires_on, leave_request_id,
                  note, created_at, entry_kind
        "#,
    )
    .bind(public_id.uuid())
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(employee_id)
    .bind(leave_type_id)
    .bind(entry_kind)
    .bind(units_milli)
    .bind(effective_date)
    .bind(expires_on)
    .bind(leave_request_id)
    .bind(note)
    .bind(source_key)
    .bind(owner_user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let dto = LeaveLedgerEntryDto {
        id: row.public_id,
        employee_id: PublicId::new(IdKind::Employee, employee_id).as_str(),
        leave_type_id: PublicId::new(IdKind::LeaveType, leave_type_id).as_str(),
        entry_kind: row.entry_kind,
        units_milli: row.units_milli,
        effective_date: row.effective_date.to_string(),
        expires_on: row.expires_on.map(|d| d.to_string()),
        leave_request_id: row
            .leave_request_id
            .map(|u| PublicId::new(IdKind::LeaveRequest, u).as_str()),
        note: row.note,
        created_at: row.created_at.to_rfc3339(),
    };

    insert_audit(
        &mut **tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.leave.ledger_post",
        "leave_ledger",
        &dto.id,
        serde_json::json!({
            "entry_kind": entry_kind,
            "units_milli": units_milli,
        }),
    )
    .await
    .map_err(internal(request_id))?;

    let _ = auth; // used above
    Ok(dto)
}

#[allow(clippy::too_many_arguments)]
async fn do_submit(
    tx: &mut Transaction<'_, Postgres>,
    _state: &AppState,
    auth: &AuthCtx,
    org_id: Uuid,
    public_id: &str,
    lt: &LeaveTypeRow,
    emp: &EmployeeRow,
    request_id: &str,
) -> Result<LeaveRequestDto, AppError> {
    let rid = parse_public_id(IdKind::LeaveRequest, public_id, request_id)?;
    let row = fetch_leave_request(tx, org_id, rid, request_id).await?;

    // Balance check for paid leave types.
    if lt.category != "unpaid" {
        let entries = load_ledger_entries(tx, org_id, emp.id, lt.id, request_id).await?;
        let bal = balance_as_of(&entries, row.start_date);
        if bal < row.units_milli {
            return Err(conflict(
                request_id,
                format!(
                    "insufficient leave balance: have {} days, need {}",
                    format_days(bal),
                    format_days(row.units_milli)
                ),
            ));
        }
    }

    let mut approval_id: Option<String> = None;
    if lt.requires_approval {
        approval_id = request_leave_approval(
            auth,
            public_id,
            &emp.display_name,
            &lt.code,
            row.units_milli,
            &row.start_date.to_string(),
            &row.end_date.to_string(),
        )
        .await;
    }

    let new_status = if lt.requires_approval {
        "pending_approval"
    } else {
        "approved"
    };

    let updated: LeaveRequestRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_leave_request SET
            status = $3, approval_id = $4,
            decided_at = CASE WHEN $3 = 'approved' THEN now() ELSE decided_at END,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {LR_COLS}
        "#
    ))
    .bind(org_id)
    .bind(rid)
    .bind(new_status)
    .bind(approval_id.as_deref())
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    if new_status == "approved" {
        insert_ledger_entry(
            tx,
            org_id,
            auth,
            updated.employee_id,
            updated.leave_type_id,
            "debit",
            -updated.units_milli,
            updated.start_date,
            None,
            Some(updated.id),
            Some("auto-approved leave"),
            Some(&format!("debit:{}", updated.public_id)),
            updated.owner_user_id,
            request_id,
        )
        .await?;
    }

    let dto = updated.into_dto();
    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "leave",
        "requested",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "employee_id": dto.employee_id,
            "status": dto.status,
            "approval_id": dto.approval_id,
        }),
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    insert_audit(
        &mut **tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.leave_request.submit",
        "leave_request",
        &dto.id,
        serde_json::json!({ "status": dto.status }),
    )
    .await
    .map_err(internal(request_id))?;

    Ok(dto)
}

async fn request_leave_approval(
    auth: &AuthCtx,
    leave_public_id: &str,
    employee_name: &str,
    leave_code: &str,
    units_milli: i32,
    start: &str,
    end: &str,
) -> Option<String> {
    let project_url =
        std::env::var("PROJECT_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8084".into());
    let url = format!(
        "{}/api/v1/operations/approvals",
        project_url.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let mut req = client.post(&url).json(&serde_json::json!({
        "subject_type": "leave_request",
        "subject_id": leave_public_id,
        "title": format!("Leave: {employee_name} ({leave_code})"),
        "summary": format!("{start} → {end} ({})", format_days(units_milli)),
        "category": leave_code,
    }));
    req = req
        .header(
            "x-companyos-dev-org-id",
            auth.ctx.org_id.to_public().as_str(),
        )
        .header(
            "x-companyos-dev-user-id",
            PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
        );
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    body.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

async fn maybe_clear_on_leave(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    let today = Utc::now().date_naive();
    let open: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::bigint FROM people_leave_request
        WHERE org_id = $1 AND employee_id = $2 AND deleted_at IS NULL
          AND status = 'approved' AND start_date <= $3 AND end_date >= $3
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .bind(today)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    if open.0 == 0 {
        sqlx::query(
            "UPDATE people_employee SET status = 'active', updated_at = now(), version = version + 1
             WHERE org_id = $1 AND id = $2 AND status = 'on_leave' AND deleted_at IS NULL",
        )
        .bind(org_id)
        .bind(employee_id)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }
    Ok(())
}

fn parse_date(s: &str, request_id: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| validation(request_id, format!("invalid date: {s}")))
}

fn parse_opt_date(raw: Option<&str>, request_id: &str) -> Result<Option<NaiveDate>, AppError> {
    match raw {
        None | Some("") => Ok(None),
        Some(s) => parse_date(s, request_id).map(Some),
    }
}
