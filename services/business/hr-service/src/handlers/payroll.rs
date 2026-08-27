//! `/api/v1/people/payroll/...` — payroll components, runs, payslips, and pay.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::employees::{fetch_employee_by_user, EmployeeRow};
use super::{conflict, crypto_err, internal, normalize_paging, not_found, parse_optional_public_id, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreatePayrollComponentRequest, CreatePayrollRunRequest, DecidePayrollRequest,
    PayrollComponentDto, PayrollComponentListResponse, PayrollRunDto, PayrollRunListQuery,
    PayrollRunListResponse, PayslipDto, PayslipLineDto, PayslipListResponse,
};

const VALID_LINE_KINDS: &[&str] = &["earning", "deduction"];
const VALID_CALC_METHODS: &[&str] = &[
    "fixed_from_comp",
    "rate_x_hours",
    "unpaid_leave_proration",
    "percent_of_gross",
    "fixed_amount",
];
const MUTABLE_RUN_STATUSES: &[&str] = &["draft", "calculated"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/people/payroll/components",
            get(list_components).post(create_component),
        )
        .route(
            "/api/v1/people/payroll/runs",
            get(list_runs).post(create_run),
        )
        .route("/api/v1/people/payroll/runs/{id}", get(get_run))
        .route(
            "/api/v1/people/payroll/runs/{id}/calculate",
            post(calculate_run),
        )
        .route("/api/v1/people/payroll/runs/{id}/submit", post(submit_run))
        .route("/api/v1/people/payroll/runs/{id}/approve", post(approve_run))
        .route("/api/v1/people/payroll/runs/{id}/decide", post(decide_run))
        .route("/api/v1/people/payroll/runs/{id}/pay", post(pay_run))
        .route("/api/v1/people/payroll/runs/{id}/adjust", post(adjust_run))
        .route(
            "/api/v1/people/payroll/runs/{id}/payslips",
            get(list_run_payslips),
        )
        .route("/api/v1/people/payroll/runs/{id}/export", get(export_run))
        .route("/api/v1/people/payroll/payslips/{id}", get(get_payslip))
        .route("/api/v1/people/me/payslips", get(list_my_payslips))
        .route("/api/v1/people/me/payslips/{id}", get(get_my_payslip))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ComponentRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    code: String,
    label: String,
    line_kind: String,
    calc_method: String,
    config_json: serde_json::Value,
    currency: Option<String>,
    is_active: bool,
    sort_order: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl ComponentRow {
    fn into_dto(self) -> PayrollComponentDto {
        PayrollComponentDto {
            id: self.public_id,
            code: self.code,
            label: self.label,
            line_kind: self.line_kind,
            calc_method: self.calc_method,
            config_json: self.config_json,
            currency: self.currency,
            is_active: self.is_active,
            sort_order: self.sort_order,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct RunRow {
    id: Uuid,
    public_id: String,
    status: String,
    period_start: NaiveDate,
    period_end: NaiveDate,
    currency: String,
    adjustment_of_run_id: Option<Uuid>,
    approval_id: Option<String>,
    journal_public_id: Option<String>,
    employee_count: i32,
    gross_minor: i64,
    deductions_minor: i64,
    net_minor: i64,
    calculated_at: Option<DateTime<Utc>>,
    approved_at: Option<DateTime<Utc>>,
    paid_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl RunRow {
    fn into_dto(self) -> PayrollRunDto {
        PayrollRunDto {
            id: self.public_id,
            status: self.status,
            period_start: self.period_start.to_string(),
            period_end: self.period_end.to_string(),
            currency: self.currency,
            adjustment_of_run_id: self
                .adjustment_of_run_id
                .map(|u| PublicId::new(IdKind::PayrollRun, u).as_str()),
            approval_id: self.approval_id,
            journal_public_id: self.journal_public_id,
            employee_count: self.employee_count,
            gross_minor: self.gross_minor,
            deductions_minor: self.deductions_minor,
            net_minor: self.net_minor,
            calculated_at: self.calculated_at.map(|t| t.to_rfc3339()),
            approved_at: self.approved_at.map(|t| t.to_rfc3339()),
            paid_at: self.paid_at.map(|t| t.to_rfc3339()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PayslipRow {
    id: Uuid,
    public_id: String,
    #[allow(dead_code)]
    run_id: Uuid,
    run_public_id: String,
    #[allow(dead_code)]
    employee_id: Uuid,
    employee_public_id: String,
    currency: String,
    gross_minor: i64,
    deductions_minor: i64,
    net_minor: i64,
    status: String,
    issued_at: Option<DateTime<Utc>>,
    #[allow(dead_code)]
    employee_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    version: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PayslipLineRow {
    public_id: String,
    line_kind: String,
    component_code: String,
    label: String,
    amount_minor: i64,
    currency: String,
    calculation_basis: serde_json::Value,
    sort_order: i32,
}

const RUN_COLS: &str = r#"
    id, public_id, status, period_start, period_end, currency,
    adjustment_of_run_id, approval_id, journal_public_id,
    employee_count, gross_minor, deductions_minor, net_minor,
    calculated_at, approved_at, paid_at, created_at, updated_at, version
"#;

const PAYSLIP_COLS: &str = r#"
    p.id, p.public_id, p.run_id, r.public_id AS run_public_id,
    p.employee_id, e.public_id AS employee_public_id,
    p.currency, p.gross_minor, p.deductions_minor, p.net_minor,
    p.status, p.issued_at, p.employee_user_id, p.created_at, p.version
"#;
/// GET /api/v1/people/payroll/components
#[utoipa::path(get, path = "/api/v1/people/payroll/components", tag = "people-payroll",
    responses((status = 200, body = PayrollComponentListResponse)))]
pub async fn list_components(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<PayrollComponentListResponse>, AppError> {
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
        perms::hr_payroll_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<ComponentRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, code, label, line_kind, calc_method, config_json,
               currency, is_active, sort_order, created_at, updated_at, version
        FROM people_payroll_component
        WHERE org_id = $1 AND deleted_at IS NULL
        ORDER BY sort_order, code
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.read",
        "payroll_component",
        "list",
        serde_json::json!({ "count": rows.len() }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(PayrollComponentListResponse {
        items: rows.into_iter().map(|r| r.into_dto()).collect(),
    }))
}

/// POST /api/v1/people/payroll/components
#[utoipa::path(post, path = "/api/v1/people/payroll/components", tag = "people-payroll",
    request_body = CreatePayrollComponentRequest,
    responses((status = 201, body = PayrollComponentDto)))]
pub async fn create_component(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreatePayrollComponentRequest>,
) -> Result<(StatusCode, Json<PayrollComponentDto>), AppError> {
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
        perms::hr_payroll_write(),
        &request_id,
    )?;

    if body.code.trim().is_empty() || body.label.trim().is_empty() {
        return Err(validation(&request_id, "code and label are required"));
    }
    if !VALID_LINE_KINDS.contains(&body.line_kind.as_str()) {
        return Err(validation(
            &request_id,
            format!("line_kind must be one of {VALID_LINE_KINDS:?}"),
        ));
    }
    if !VALID_CALC_METHODS.contains(&body.calc_method.as_str()) {
        return Err(validation(
            &request_id,
            format!("calc_method must be one of {VALID_CALC_METHODS:?}"),
        ));
    }
    let currency = match body.currency.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            let c: Currency = s
                .parse()
                .map_err(|_| validation(&request_id, "invalid currency"))?;
            Some(c.as_str().to_string())
        }
        _ => None,
    };

    let id_uuid = companyos_ids::new_uuid_v7();
    let public_id = PublicId::new(IdKind::PayrollComponent, id_uuid);
    let config_json = body.config_json.unwrap_or_else(|| serde_json::json!({}));
    let sort_order = body.sort_order.unwrap_or(100);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO people_payroll_component (
            id, org_id, public_id, code, label, line_kind, calc_method,
            config_json, currency, sort_order
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        "#,
    )
    .bind(id_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.code.trim())
    .bind(body.label.trim())
    .bind(&body.line_kind)
    .bind(&body.calc_method)
    .bind(&config_json)
    .bind(currency.as_deref())
    .bind(sort_order)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row: ComponentRow = sqlx::query_as(
        r#"
        SELECT id, public_id, code, label, line_kind, calc_method, config_json,
               currency, is_active, sort_order, created_at, updated_at, version
        FROM people_payroll_component WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(id_uuid)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.component.create",
        "payroll_component",
        &public_id.as_str(),
        serde_json::json!({ "code": body.code.trim(), "line_kind": body.line_kind }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = row.into_dto();
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/people/payroll/runs
#[utoipa::path(get, path = "/api/v1/people/payroll/runs", tag = "people-payroll",
    params(PayrollRunListQuery),
    responses((status = 200, body = PayrollRunListResponse)))]
pub async fn list_runs(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<PayrollRunListQuery>,
) -> Result<Json<PayrollRunListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb =
        QueryBuilder::new("SELECT COUNT(*)::bigint FROM people_payroll_run WHERE org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND deleted_at IS NULL");
    if let Some(ref st) = q.status {
        count_qb.push(" AND status = ");
        count_qb.push_bind(st);
    }
    let total: (i64,) = count_qb
        .build_query_as()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut list_qb = QueryBuilder::new(format!(
        "SELECT {RUN_COLS} FROM people_payroll_run WHERE org_id = "
    ));
    list_qb.push_bind(org_id);
    list_qb.push(" AND deleted_at IS NULL");
    if let Some(ref st) = q.status {
        list_qb.push(" AND status = ");
        list_qb.push_bind(st);
    }
    list_qb.push(" ORDER BY period_start DESC, created_at DESC LIMIT ");
    list_qb.push_bind(limit);
    list_qb.push(" OFFSET ");
    list_qb.push_bind(offset);

    let rows: Vec<RunRow> = list_qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.read",
        "payroll_run",
        "list",
        serde_json::json!({ "count": rows.len() }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(PayrollRunListResponse {
        items: rows.into_iter().map(|r| r.into_dto()).collect(),
        total: total.0,
    }))
}
/// POST /api/v1/people/payroll/runs
#[utoipa::path(post, path = "/api/v1/people/payroll/runs", tag = "people-payroll",
    request_body = CreatePayrollRunRequest,
    responses((status = 201, body = PayrollRunDto)))]
pub async fn create_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreatePayrollRunRequest>,
) -> Result<(StatusCode, Json<PayrollRunDto>), AppError> {
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
        perms::hr_payroll_write(),
        &request_id,
    )?;

    let period_start = parse_date(&body.period_start, &request_id)?;
    let period_end = parse_date(&body.period_end, &request_id)?;
    if period_end < period_start {
        return Err(validation(
            &request_id,
            "period_end must be on or after period_start",
        ));
    }
    let currency: Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let adjustment_of = parse_optional_public_id(
        IdKind::PayrollRun,
        body.adjustment_of_run_id.as_deref(),
        &request_id,
    )?;
    if let Some(orig_id) = adjustment_of {
        let orig: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT status FROM people_payroll_run
            WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
            "#,
        )
        .bind(org_id)
        .bind(orig_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        let Some((status,)) = orig else {
            return Err(not_found(&request_id, "original payroll run"));
        };
        if status != "approved" && status != "paid" {
            return Err(conflict(
                &request_id,
                "adjustment_of_run_id must reference an approved or paid run",
            ));
        }
    }

    let id_uuid = companyos_ids::new_uuid_v7();
    let public_id = PublicId::new(IdKind::PayrollRun, id_uuid);
    let owner = auth.ctx.actor.user_id;

    sqlx::query(
        r#"
        INSERT INTO people_payroll_run (
            id, org_id, public_id, status, period_start, period_end, currency,
            adjustment_of_run_id, created_by, owner_user_id
        ) VALUES ($1,$2,$3,'draft',$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(period_start)
    .bind(period_end)
    .bind(currency.as_str())
    .bind(adjustment_of)
    .bind(auth.ctx.actor.user_id)
    .bind(owner)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_run(&mut tx, org_id, id_uuid, &request_id).await?;
    let dto = row.into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "payroll_run",
        "drafted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "period_start": dto.period_start,
            "period_end": dto.period_end,
            "currency": dto.currency,
            "adjustment_of_run_id": dto.adjustment_of_run_id,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.run.create",
        "payroll_run",
        &dto.id,
        serde_json::json!({
            "period_start": dto.period_start,
            "period_end": dto.period_end,
            "currency": dto.currency,
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/people/payroll/runs/{id}
#[utoipa::path(get, path = "/api/v1/people/payroll/runs/{id}", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, body = PayrollRunDto), (status = 404)))]
pub async fn get_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<PayrollRunDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let run_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_run(&mut tx, org_id, run_id, &request_id).await?;
    let dto = row.into_dto();

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.read",
        "payroll_run",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
/// POST /api/v1/people/payroll/runs/{id}/calculate
#[utoipa::path(post, path = "/api/v1/people/payroll/runs/{id}/calculate", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, body = PayrollRunDto)))]
pub async fn calculate_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let run_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_run(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) =
            idempotency::get(&mut *tx, org_id, "payroll.calculate", &key)
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

    let run = fetch_run(&mut tx, org_id, run_id, &request_id).await?;
    if run.status == "approved" || run.status == "paid" {
        return Err(conflict(
            &request_id,
            "approved or paid payroll runs cannot be recalculated",
        ));
    }
    if !MUTABLE_RUN_STATUSES.contains(&run.status.as_str()) {
        return Err(conflict(
            &request_id,
            format!("payroll run status {} cannot be calculated", run.status),
        ));
    }

    if run.status == "calculated" {
        clear_run_payslips(&mut tx, org_id, run_id, &request_id).await?;
    }

    ensure_default_components(&mut tx, org_id, &request_id).await?;

    let components: Vec<ComponentRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, code, label, line_kind, calc_method, config_json,
               currency, is_active, sort_order, created_at, updated_at, version
        FROM people_payroll_component
        WHERE org_id = $1 AND deleted_at IS NULL AND is_active = true
        ORDER BY sort_order, code
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let employees: Vec<EmployeeRow> = sqlx::query_as(&format!(
        "SELECT {} FROM people_employee WHERE org_id = $1 AND deleted_at IS NULL AND status IN ('active','on_leave')",
        super::employees::EMPLOYEE_COLUMNS
    ))
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let working_days = count_weekdays(run.period_start, run.period_end);
    if working_days == 0 {
        return Err(validation(
            &request_id,
            "pay period has no working days",
        ));
    }

    let mut total_gross: i64 = 0;
    let mut total_deductions: i64 = 0;
    let mut total_net: i64 = 0;
    let mut emp_count = 0i32;

    for emp in &employees {
        let salary_ct: Option<(Vec<u8>, String)> = sqlx::query_as(
            r#"
            SELECT amount_minor_ciphertext, currency
            FROM people_compensation_component
            WHERE org_id = $1 AND employee_id = $2 AND component_type = 'base_salary'
              AND deleted_at IS NULL
              AND effective_from <= $3
              AND (effective_to IS NULL OR effective_to >= $3)
            ORDER BY effective_from DESC
            LIMIT 1
            "#,
        )
        .bind(org_id)
        .bind(emp.id)
        .bind(run.period_end)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        let Some((ct, comp_currency)) = salary_ct else {
            continue;
        };
        if comp_currency != run.currency {
            continue;
        }

        let salary = state
            .encryptor
            .decrypt_i64(&ct)
            .map_err(|e| crypto_err(&request_id, e))?;

        insert_audit(
            &mut *tx,
            org_id,
            auth.ctx.actor.user_id,
            auth.ctx.actor.on_behalf_of,
            auth.ctx.actor.is_ai,
            "hr.payroll.calc_read_compensation",
            "employee",
            &emp.public_id,
            serde_json::json!({ "run_id": run.public_id }),
        )
        .await
        .map_err(internal(&request_id))?;

        let unpaid_milli = unpaid_leave_milli(
            &mut tx,
            org_id,
            emp.id,
            run.period_start,
            run.period_end,
            &request_id,
        )
        .await?;

        let ot_hours_milli = sum_attendance_hours_milli(
            &mut tx,
            org_id,
            emp.id,
            run.period_start,
            run.period_end,
            &request_id,
        )
        .await?;

        let mut slip_gross: i64 = 0;
        let mut slip_deductions: i64 = 0;
        let mut lines: Vec<(String, String, String, i64, serde_json::Value, i32)> = Vec::new();

        for comp in &components {
            if comp.calc_method == "percent_of_gross" {
                continue;
            }
            let (amount, basis) = compute_line_amount(
                comp,
                salary,
                unpaid_milli,
                working_days,
                ot_hours_milli,
            );
            if amount == 0 {
                continue;
            }
            match comp.calc_method.as_str() {
                "fixed_from_comp" | "rate_x_hours" => slip_gross += amount,
                "unpaid_leave_proration" => slip_gross = slip_gross.saturating_sub(amount),
                "fixed_amount" if comp.line_kind == "earning" => slip_gross += amount,
                "fixed_amount" => slip_deductions += amount,
                _ if comp.line_kind == "earning" => slip_gross += amount,
                _ => slip_deductions += amount,
            }
            lines.push((
                comp.line_kind.clone(),
                comp.code.clone(),
                comp.label.clone(),
                amount,
                basis,
                comp.sort_order,
            ));
        }

        for comp in &components {
            if comp.calc_method != "percent_of_gross" || comp.line_kind != "deduction" {
                continue;
            }
            let bps = comp
                .config_json
                .get("percent_bps")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let amount = slip_gross.saturating_mul(bps) / 10_000;
            if amount == 0 {
                continue;
            }
            slip_deductions += amount;
            lines.push((
                "deduction".into(),
                comp.code.clone(),
                comp.label.clone(),
                amount,
                serde_json::json!({
                    "method": "percent_of_gross",
                    "inputs": { "gross_minor": slip_gross, "percent_bps": bps }
                }),
                comp.sort_order,
            ));
        }
        let slip_net = slip_gross - slip_deductions;

        if slip_gross == 0 && slip_deductions == 0 {
            continue;
        }

        let payslip_id = companyos_ids::new_uuid_v7();
        let payslip_public = PublicId::new(IdKind::Payslip, payslip_id);

        sqlx::query(
            r#"
            INSERT INTO people_payslip (
                id, org_id, public_id, run_id, employee_id, currency,
                gross_minor, deductions_minor, net_minor, status,
                employee_user_id, owner_user_id
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'draft',$10,$11)
            "#,
        )
        .bind(payslip_id)
        .bind(org_id)
        .bind(payslip_public.as_str())
        .bind(run_id)
        .bind(emp.id)
        .bind(&run.currency)
        .bind(slip_gross)
        .bind(slip_deductions)
        .bind(slip_net)
        .bind(emp.user_id)
        .bind(emp.owner_user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        for (line_kind, code, label, amount, basis, sort_order) in lines {
            let line_id = companyos_ids::new_uuid_v7();
            let line_kind_id = if line_kind == "earning" {
                IdKind::PayrollEarningLine
            } else {
                IdKind::PayrollDeductionLine
            };
            let line_public = PublicId::new(line_kind_id, line_id);
            sqlx::query(
                r#"
                INSERT INTO people_payslip_line (
                    id, org_id, public_id, payslip_id, run_id, line_kind,
                    component_code, label, amount_minor, currency,
                    calculation_basis, sort_order
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
                "#,
            )
            .bind(line_id)
            .bind(org_id)
            .bind(line_public.as_str())
            .bind(payslip_id)
            .bind(run_id)
            .bind(&line_kind)
            .bind(&code)
            .bind(&label)
            .bind(amount)
            .bind(&run.currency)
            .bind(&basis)
            .bind(sort_order)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        }

        total_gross += slip_gross;
        total_deductions += slip_deductions;
        total_net += slip_net;
        emp_count += 1;
    }

    let updated: RunRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_payroll_run SET
            status = 'calculated', calculated_at = now(),
            employee_count = $3, gross_minor = $4, deductions_minor = $5, net_minor = $6,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {RUN_COLS}
        "#
    ))
    .bind(org_id)
    .bind(run_id)
    .bind(emp_count)
    .bind(total_gross)
    .bind(total_deductions)
    .bind(total_net)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = updated.into_dto();
    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "payroll_run",
        "calculated",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "employee_count": dto.employee_count,
            "status": dto.status,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.calculate",
        "payroll_run",
        &dto.id,
        serde_json::json!({ "employee_count": emp_count }),
    )
    .await
    .map_err(internal(&request_id))?;

    let body_json = serde_json::to_value(&dto).map_err(|e| {
        AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
    })?;
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "payroll.calculate", &key, 200, body_json.clone())
            .await
            .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::OK, Json(body_json)).into_response())
}
/// POST /api/v1/people/payroll/runs/{id}/submit
#[utoipa::path(post, path = "/api/v1/people/payroll/runs/{id}/submit", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, body = PayrollRunDto)))]
pub async fn submit_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<PayrollRunDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let run_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let run = fetch_run(&mut tx, org_id, run_id, &request_id).await?;
    reject_immutable(&run.status, &request_id)?;
    if run.status != "calculated" {
        return Err(conflict(
            &request_id,
            format!("payroll run status {} is not calculated", run.status),
        ));
    }

    let approval_id = request_payroll_approval(
        &auth,
        &run.public_id,
        &run.period_start.to_string(),
        &run.period_end.to_string(),
        run.employee_count,
    )
    .await;

    let updated: RunRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_payroll_run SET
            status = 'in_review', approval_id = $3,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {RUN_COLS}
        "#
    ))
    .bind(org_id)
    .bind(run_id)
    .bind(approval_id.as_deref())
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = updated.into_dto();
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.submit",
        "payroll_run",
        &dto.id,
        serde_json::json!({ "approval_id": dto.approval_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/people/payroll/runs/{id}/approve
#[utoipa::path(post, path = "/api/v1/people/payroll/runs/{id}/approve", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, body = PayrollRunDto)))]
pub async fn approve_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let run_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_approve(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) =
            idempotency::get(&mut *tx, org_id, "payroll.approve", &key)
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

    let run = fetch_run(&mut tx, org_id, run_id, &request_id).await?;
    if run.status == "approved" || run.status == "paid" {
        let dto = run.into_dto();
        let body_json = serde_json::to_value(&dto).map_err(|e| {
            AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
        })?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((StatusCode::OK, Json(body_json)).into_response());
    }
    if run.status != "in_review" && run.status != "calculated" {
        return Err(conflict(
            &request_id,
            format!("payroll run status {} cannot be approved", run.status),
        ));
    }

    let dto = do_approve_run(&mut tx, &auth, org_id, run_id, None, &request_id).await?;

    let body_json = serde_json::to_value(&dto).map_err(|e| {
        AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
    })?;
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "payroll.approve", &key, 200, body_json.clone())
            .await
            .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::OK, Json(body_json)).into_response())
}

/// POST /api/v1/people/payroll/runs/{id}/decide
#[utoipa::path(post, path = "/api/v1/people/payroll/runs/{id}/decide", tag = "people-payroll",
    request_body = DecidePayrollRequest,
    params(("id" = String, Path)),
    responses((status = 200, body = PayrollRunDto)))]
pub async fn decide_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<DecidePayrollRequest>,
) -> Result<Json<PayrollRunDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let run_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_approve(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let run = fetch_run(&mut tx, org_id, run_id, &request_id).await?;
    if run.status == "approved" || run.status == "paid" {
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(run.into_dto()));
    }
    if run.status != "in_review" && run.status != "calculated" {
        return Err(conflict(
            &request_id,
            format!("payroll run status {} cannot be decided", run.status),
        ));
    }

    let dto = if body.approve {
        do_approve_run(&mut tx, &auth, org_id, run_id, body.note.as_deref(), &request_id).await?
    } else {
        let updated: RunRow = sqlx::query_as(&format!(
            r#"
            UPDATE people_payroll_run SET
                status = 'calculated', version = version + 1, updated_at = now()
            WHERE org_id = $1 AND id = $2
            RETURNING {RUN_COLS}
            "#
        ))
        .bind(org_id)
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        updated.into_dto()
    };

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
/// POST /api/v1/people/payroll/runs/{id}/pay
#[utoipa::path(post, path = "/api/v1/people/payroll/runs/{id}/pay", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, body = PayrollRunDto)))]
pub async fn pay_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let run_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_run(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) = idempotency::get(&mut *tx, org_id, "payroll.pay", &key)
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

    let run = fetch_run(&mut tx, org_id, run_id, &request_id).await?;
    if run.status == "paid" {
        let dto = run.into_dto();
        let body_json = serde_json::to_value(&dto).map_err(|e| {
            AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
        })?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((StatusCode::OK, Json(body_json)).into_response());
    }
    if run.status != "approved" {
        return Err(conflict(
            &request_id,
            format!("payroll run status {} is not approved", run.status),
        ));
    }

    let idem_key = idempotency::header_key(&headers);
    let (journal_public_id, journal_entry_id) = post_payroll_journal(
        &auth,
        &run,
        idem_key.as_deref(),
        &request_id,
    )
    .await?;

    let updated: RunRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_payroll_run SET
            status = 'paid', paid_at = now(),
            journal_public_id = $3, journal_entry_id = $4,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {RUN_COLS}
        "#
    ))
    .bind(org_id)
    .bind(run_id)
    .bind(&journal_public_id)
    .bind(journal_entry_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let payslips: Vec<PayslipRow> = sqlx::query_as(&format!(
        r#"
        SELECT {PAYSLIP_COLS}
        FROM people_payslip p
        JOIN people_payroll_run r ON r.id = p.run_id AND r.org_id = p.org_id
        JOIN people_employee e ON e.id = p.employee_id AND e.org_id = p.org_id
        WHERE p.org_id = $1 AND p.run_id = $2 AND p.deleted_at IS NULL
        "#
    ))
    .bind(org_id)
    .bind(run_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for slip in &payslips {
        sqlx::query(
            r#"
            UPDATE people_payslip SET status = 'issued', issued_at = now(),
                version = version + 1, updated_at = now()
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(slip.id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        let envelope = EventEnvelope::new(
            auth.ctx.org_id,
            Context::People,
            "payslip",
            "issued",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "id": slip.public_id,
                "run_id": slip.run_public_id,
                "employee_id": slip.employee_public_id,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &envelope)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }

    let dto = updated.into_dto();
    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "payroll_run",
        "paid",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "journal_public_id": dto.journal_public_id,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.pay",
        "payroll_run",
        &dto.id,
        serde_json::json!({
            "journal_public_id": dto.journal_public_id,
            "payslip_count": payslips.len(),
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let body_json = serde_json::to_value(&dto).map_err(|e| {
        AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
    })?;
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "payroll.pay", &key, 200, body_json.clone())
            .await
            .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::OK, Json(body_json)).into_response())
}

/// POST /api/v1/people/payroll/runs/{id}/adjust
#[utoipa::path(post, path = "/api/v1/people/payroll/runs/{id}/adjust", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 201, body = PayrollRunDto)))]
pub async fn adjust_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<PayrollRunDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let orig_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let orig = fetch_run(&mut tx, org_id, orig_id, &request_id).await?;
    if orig.status != "approved" && orig.status != "paid" {
        return Err(conflict(
            &request_id,
            "adjustment source run must be approved or paid",
        ));
    }

    let id_uuid = companyos_ids::new_uuid_v7();
    let public_id = PublicId::new(IdKind::PayrollRun, id_uuid);

    sqlx::query(
        r#"
        INSERT INTO people_payroll_run (
            id, org_id, public_id, status, period_start, period_end, currency,
            adjustment_of_run_id, created_by, owner_user_id
        ) VALUES ($1,$2,$3,'draft',$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(orig.period_start)
    .bind(orig.period_end)
    .bind(&orig.currency)
    .bind(orig_id)
    .bind(auth.ctx.actor.user_id)
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_run(&mut tx, org_id, id_uuid, &request_id).await?;
    let dto = row.into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "payroll_run",
        "drafted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "adjustment_of_run_id": PublicId::new(IdKind::PayrollRun, orig_id).as_str(),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.adjust",
        "payroll_run",
        &dto.id,
        serde_json::json!({ "adjustment_of_run_id": orig.public_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}
/// GET /api/v1/people/payroll/runs/{id}/payslips
#[utoipa::path(get, path = "/api/v1/people/payroll/runs/{id}/payslips", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, body = PayslipListResponse)))]
pub async fn list_run_payslips(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<PayslipListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let run_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let _run = fetch_run(&mut tx, org_id, run_id, &request_id).await?;
    let slips = load_payslips_for_run(&mut tx, org_id, run_id, true, &request_id).await?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.read",
        "payroll_run",
        &id,
        serde_json::json!({ "payslip_count": slips.len(), "resource": "payslips" }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(PayslipListResponse { items: slips }))
}

/// GET /api/v1/people/payroll/runs/{id}/export
#[utoipa::path(get, path = "/api/v1/people/payroll/runs/{id}/export", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, description = "ACH-style CSV payment file")))]
pub async fn export_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let run_id = parse_public_id(IdKind::PayrollRun, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let run = fetch_run(&mut tx, org_id, run_id, &request_id).await?;
    let rows: Vec<(String, String, i64, String, String)> = sqlx::query_as(
        r#"
        SELECT p.public_id, e.public_id, p.net_minor, p.currency, e.display_name
        FROM people_payslip p
        JOIN people_employee e ON e.id = p.employee_id AND e.org_id = p.org_id
        WHERE p.org_id = $1 AND p.run_id = $2 AND p.deleted_at IS NULL
        ORDER BY e.display_name
        "#,
    )
    .bind(org_id)
    .bind(run_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.read",
        "payroll_run",
        &run.public_id,
        serde_json::json!({ "export": "ach_csv", "row_count": rows.len() }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    let mut csv = String::from("payslip_id,employee_id,employee_name,net_minor,currency\n");
    for (payslip_id, emp_id, net, currency, name) in rows {
        let escaped_name = name.replace('"', "\"\"");
        csv.push_str(&format!(
            "{payslip_id},{emp_id},\"{escaped_name}\",{net},{currency}\n"
        ));
    }

    Ok((
        StatusCode::OK,
        [("content-type", "text/csv")],
        csv,
    ))
}

/// GET /api/v1/people/payroll/payslips/{id}
#[utoipa::path(get, path = "/api/v1/people/payroll/payslips/{id}", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, body = PayslipDto), (status = 403), (status = 404)))]
pub async fn get_payslip(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<PayslipDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let slip_id = parse_public_id(IdKind::Payslip, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_payroll_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let dto = load_payslip_dto(&mut tx, org_id, slip_id, true, &request_id).await?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payslip.read",
        "payslip",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// GET /api/v1/people/me/payslips
#[utoipa::path(get, path = "/api/v1/people/me/payslips", tag = "people-payroll",
    responses((status = 200, body = PayslipListResponse)))]
pub async fn list_my_payslips(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<PayslipListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let _membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let emp = fetch_employee_by_user(&mut tx, org_id, actor)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee profile"))?;

    let rows: Vec<PayslipRow> = sqlx::query_as(&format!(
        r#"
        SELECT {PAYSLIP_COLS}
        FROM people_payslip p
        JOIN people_payroll_run r ON r.id = p.run_id AND r.org_id = p.org_id
        JOIN people_employee e ON e.id = p.employee_id AND e.org_id = p.org_id
        WHERE p.org_id = $1 AND p.employee_id = $2 AND p.deleted_at IS NULL
        ORDER BY p.created_at DESC
        "#
    ))
    .bind(org_id)
    .bind(emp.id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(
            load_payslip_dto(&mut tx, org_id, row.id, true, &request_id)
                .await?,
        );
        insert_audit(
            &mut *tx,
            org_id,
            auth.ctx.actor.user_id,
            auth.ctx.actor.on_behalf_of,
            auth.ctx.actor.is_ai,
            "hr.payslip.read",
            "payslip",
            &row.public_id,
            serde_json::json!({ "self_service": true }),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(PayslipListResponse { items }))
}

/// GET /api/v1/people/me/payslips/{id}
#[utoipa::path(get, path = "/api/v1/people/me/payslips/{id}", tag = "people-payroll",
    params(("id" = String, Path)),
    responses((status = 200, body = PayslipDto), (status = 403), (status = 404)))]
pub async fn get_my_payslip(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<PayslipDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let slip_id = parse_public_id(IdKind::Payslip, &id, &request_id)?;
    let actor = auth.ctx.actor.user_id;

    let _membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let slip: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT p.id FROM people_payslip p
        WHERE p.org_id = $1 AND p.id = $2 AND p.deleted_at IS NULL
          AND p.employee_user_id = $3
        "#,
    )
    .bind(org_id)
    .bind(slip_id)
    .bind(actor)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if slip.is_none() {
        let emp = fetch_employee_by_user(&mut tx, org_id, actor)
            .await
            .map_err(internal(&request_id))?;
        if let Some(emp) = emp {
            let owned: Option<(Uuid,)> = sqlx::query_as(
                r#"
                SELECT p.id FROM people_payslip p
                WHERE p.org_id = $1 AND p.id = $2 AND p.employee_id = $3 AND p.deleted_at IS NULL
                "#,
            )
            .bind(org_id)
            .bind(slip_id)
            .bind(emp.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            if owned.is_none() {
                return Err(AppError::new(
                    ErrorCode::Forbidden,
                    request_id,
                    "payslip belongs to another employee",
                ));
            }
        } else {
            return Err(not_found(&request_id, "payslip"));
        }
    }

    let dto = load_payslip_dto(&mut tx, org_id, slip_id, true, &request_id).await?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payslip.read",
        "payslip",
        &dto.id,
        serde_json::json!({ "self_service": true }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
async fn fetch_run(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    run_id: Uuid,
    request_id: &str,
) -> Result<RunRow, AppError> {
    sqlx::query_as(&format!(
        "SELECT {RUN_COLS} FROM people_payroll_run WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "payroll run"))
}

fn reject_immutable(status: &str, request_id: &str) -> Result<(), AppError> {
    if status == "approved" || status == "paid" {
        return Err(conflict(
            request_id,
            "approved or paid payroll runs are immutable",
        ));
    }
    Ok(())
}

async fn clear_run_payslips(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    run_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM people_payslip_line WHERE org_id = $1 AND run_id = $2")
        .bind(org_id)
        .bind(run_id)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    sqlx::query("DELETE FROM people_payslip WHERE org_id = $1 AND run_id = $2")
        .bind(org_id)
        .bind(run_id)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    Ok(())
}

async fn ensure_default_components(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM people_payroll_component WHERE org_id = $1 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    if count.0 > 0 {
        return Ok(());
    }

    let defaults: [(&str, &str, &str, &str, serde_json::Value, i32); 4] = [
        (
            "salary",
            "Base salary",
            "earning",
            "fixed_from_comp",
            serde_json::json!({}),
            10,
        ),
        (
            "unpaid_leave",
            "Unpaid leave",
            "deduction",
            "unpaid_leave_proration",
            serde_json::json!({}),
            20,
        ),
        (
            "tax",
            "Payroll tax",
            "deduction",
            "percent_of_gross",
            serde_json::json!({ "percent_bps": 1000 }),
            30,
        ),
        (
            "overtime",
            "Overtime",
            "earning",
            "rate_x_hours",
            serde_json::json!({ "overtime_rate_minor": 0 }),
            40,
        ),
    ];

    for (code, label, line_kind, calc_method, config, sort_order) in defaults {
        let id = companyos_ids::new_uuid_v7();
        let public_id = PublicId::new(IdKind::PayrollComponent, id);
        sqlx::query(
            r#"
            INSERT INTO people_payroll_component (
                id, org_id, public_id, code, label, line_kind, calc_method,
                config_json, sort_order
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(public_id.as_str())
        .bind(code)
        .bind(label)
        .bind(line_kind)
        .bind(calc_method)
        .bind(&config)
        .bind(sort_order)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }
    Ok(())
}

fn count_weekdays(start: NaiveDate, end: NaiveDate) -> i32 {
    let mut count = 0i32;
    let mut d = start;
    while d <= end {
        let wd = d.weekday().num_days_from_monday();
        if wd < 5 {
            count += 1;
        }
        d += Duration::days(1);
    }
    count
}

async fn unpaid_leave_milli(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    period_start: NaiveDate,
    period_end: NaiveDate,
    request_id: &str,
) -> Result<i32, AppError> {
    let row: (i64,) = sqlx::query_as(
        r#"
        SELECT COALESCE(SUM(lr.units_milli), 0)::bigint
        FROM people_leave_request lr
        JOIN people_leave_type lt ON lt.id = lr.leave_type_id AND lt.org_id = lr.org_id
        WHERE lr.org_id = $1 AND lr.employee_id = $2 AND lr.deleted_at IS NULL
          AND lr.status = 'approved' AND lt.category = 'unpaid'
          AND lr.start_date <= $4 AND lr.end_date >= $3
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .bind(period_start)
    .bind(period_end)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(row.0.clamp(0, i64::from(i32::MAX)) as i32)
}

async fn sum_attendance_hours_milli(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    period_start: NaiveDate,
    period_end: NaiveDate,
    request_id: &str,
) -> Result<i64, AppError> {
    #[derive(sqlx::FromRow)]
    struct AttRow {
        entry_kind: String,
        recorded_at: DateTime<Utc>,
    }
    let rows: Vec<AttRow> = sqlx::query_as(
        r#"
        SELECT entry_kind, recorded_at FROM people_attendance
        WHERE org_id = $1 AND employee_id = $2
          AND local_date >= $3 AND local_date <= $4
        ORDER BY recorded_at ASC
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .bind(period_start)
    .bind(period_end)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let mut total_ms: i64 = 0;
    let mut open_in: Option<DateTime<Utc>> = None;
    for row in rows {
        match row.entry_kind.as_str() {
            "check_in" => open_in = Some(row.recorded_at),
            "check_out" => {
                if let Some(start) = open_in.take() {
                    let delta = row.recorded_at.signed_duration_since(start);
                    if delta.num_milliseconds() > 0 {
                        total_ms += delta.num_milliseconds();
                    }
                }
            }
            _ => {}
        }
    }
    Ok(total_ms / 3_600_000 * 1000)
}

fn compute_line_amount(
    comp: &ComponentRow,
    salary: i64,
    unpaid_milli: i32,
    working_days: i32,
    ot_hours_milli: i64,
) -> (i64, serde_json::Value) {
    match comp.calc_method.as_str() {
        "fixed_from_comp" => (
            salary,
            serde_json::json!({
                "method": "fixed_from_comp",
                "inputs": { "component_type": "base_salary", "period_salary_minor": salary }
            }),
        ),
        "unpaid_leave_proration" => {
            let denom = i64::from(working_days) * 1000;
            let reduction = if denom > 0 {
                salary.saturating_mul(i64::from(unpaid_milli)) / denom
            } else {
                0
            };
            (
                reduction,
                serde_json::json!({
                    "method": "unpaid_leave_proration",
                    "inputs": {
                        "salary_minor": salary,
                        "unpaid_milli": unpaid_milli,
                        "working_days": working_days
                    }
                }),
            )
        }
        "rate_x_hours" => {
            let rate = comp
                .config_json
                .get("overtime_rate_minor")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let amount = rate.saturating_mul(ot_hours_milli) / 1000;
            (
                amount,
                serde_json::json!({
                    "method": "rate_x_hours",
                    "inputs": {
                        "overtime_rate_minor": rate,
                        "hours_milli": ot_hours_milli
                    }
                }),
            )
        }
        "fixed_amount" => {
            let amount = comp
                .config_json
                .get("fixed_amount_minor")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            (
                amount,
                serde_json::json!({
                    "method": "fixed_amount",
                    "inputs": { "fixed_amount_minor": amount }
                }),
            )
        }
        "percent_of_gross" => (0, serde_json::json!({ "method": "percent_of_gross", "inputs": {} })),
        _ => (0, serde_json::json!({ "method": comp.calc_method, "inputs": {} })),
    }
}

async fn do_approve_run(
    tx: &mut Transaction<'_, Postgres>,
    auth: &AuthCtx,
    org_id: Uuid,
    run_id: Uuid,
    _note: Option<&str>,
    request_id: &str,
) -> Result<PayrollRunDto, AppError> {
    let updated: RunRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_payroll_run SET
            status = 'approved', approved_at = now(),
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {RUN_COLS}
        "#
    ))
    .bind(org_id)
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let dto = updated.into_dto();
    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "payroll_run",
        "approved",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id }),
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.to_string(), e.to_string()))?;

    insert_audit(
        &mut **tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.payroll.approve",
        "payroll_run",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(request_id))?;

    Ok(dto)
}

async fn post_payroll_journal(
    auth: &AuthCtx,
    run: &RunRow,
    idem_key: Option<&str>,
    request_id: &str,
) -> Result<(String, Option<Uuid>), AppError> {
    let finance_url = std::env::var("FINANCE_SERVICE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8083".into());
    let url = format!(
        "{}/api/v1/finance/journals",
        finance_url.trim_end_matches('/')
    );

    let mut lines = Vec::new();
    if run.gross_minor > 0 {
        lines.push(serde_json::json!({
            "account_code": "5100",
            "debit_minor": run.gross_minor,
            "credit_minor": 0,
            "memo": "Wages"
        }));
    }
    if run.deductions_minor > 0 {
        lines.push(serde_json::json!({
            "account_code": "2300",
            "debit_minor": 0,
            "credit_minor": run.deductions_minor,
            "memo": "Deductions"
        }));
    }
    if run.net_minor > 0 {
        lines.push(serde_json::json!({
            "account_code": "2400",
            "debit_minor": 0,
            "credit_minor": run.net_minor,
            "memo": "Net pay"
        }));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let mut req = client.post(&url).json(&serde_json::json!({
        "source_type": "payroll",
        "source_id": run.id.to_string(),
        "currency": run.currency,
        "memo": format!("Payroll {} — {}", run.period_start, run.period_end),
        "lines": lines,
    }));
    req = req
        .header(
            "x-companyos-dev-org-id",
            auth.ctx.org_id.to_public().as_str(),
        )
        .header(
            "x-companyos-dev-user-id",
            PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
        )
        .header(
            "x-companyos-on-behalf-of",
            PublicId::new(IdKind::User, auth.ctx.actor.on_behalf_of).as_str(),
        );
    if let Some(key) = idem_key {
        req = req.header("idempotency-key", key);
    }

    let resp = req.send().await.map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("finance journal request failed: {e}"),
        )
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("finance journal post failed: {status} {body}"),
        ));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        AppError::new(ErrorCode::Internal, request_id, e.to_string())
    })?;
    let journal_public_id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id,
                "finance journal response missing id",
            )
        })?
        .to_string();

    let journal_entry_id = journal_public_id
        .strip_prefix("jrn_")
        .and_then(|s| Uuid::parse_str(s).ok());

    Ok((journal_public_id, journal_entry_id))
}

async fn request_payroll_approval(
    auth: &AuthCtx,
    run_public_id: &str,
    period_start: &str,
    period_end: &str,
    employee_count: i32,
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
        "subject_type": "payroll_run",
        "subject_id": run_public_id,
        "title": format!("Payroll: {period_start} → {period_end}"),
        "summary": format!("{employee_count} employees"),
        "category": "payroll",
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

async fn load_payslip_lines(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    payslip_id: Uuid,
    request_id: &str,
) -> Result<Vec<PayslipLineDto>, AppError> {
    let rows: Vec<PayslipLineRow> = sqlx::query_as(
        r#"
        SELECT public_id, line_kind, component_code, label, amount_minor,
               currency, calculation_basis, sort_order
        FROM people_payslip_line
        WHERE org_id = $1 AND payslip_id = $2
        ORDER BY sort_order, component_code
        "#,
    )
    .bind(org_id)
    .bind(payslip_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok(rows
        .into_iter()
        .map(|r| PayslipLineDto {
            id: r.public_id,
            line_kind: r.line_kind,
            component_code: r.component_code,
            label: r.label,
            amount_minor: r.amount_minor,
            currency: r.currency,
            calculation_basis: r.calculation_basis,
            sort_order: r.sort_order,
        })
        .collect())
}

async fn load_payslip_dto(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    payslip_id: Uuid,
    with_lines: bool,
    request_id: &str,
) -> Result<PayslipDto, AppError> {
    let row: PayslipRow = sqlx::query_as(&format!(
        r#"
        SELECT {PAYSLIP_COLS}
        FROM people_payslip p
        JOIN people_payroll_run r ON r.id = p.run_id AND r.org_id = p.org_id
        JOIN people_employee e ON e.id = p.employee_id AND e.org_id = p.org_id
        WHERE p.org_id = $1 AND p.id = $2 AND p.deleted_at IS NULL
        "#
    ))
    .bind(org_id)
    .bind(payslip_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "payslip"))?;

    let lines = if with_lines {
        load_payslip_lines(tx, org_id, payslip_id, request_id).await?
    } else {
        Vec::new()
    };

    Ok(PayslipDto {
        id: row.public_id,
        run_id: row.run_public_id,
        employee_id: row.employee_public_id,
        currency: row.currency,
        gross_minor: row.gross_minor,
        deductions_minor: row.deductions_minor,
        net_minor: row.net_minor,
        status: row.status,
        issued_at: row.issued_at.map(|t| t.to_rfc3339()),
        lines,
        created_at: row.created_at.to_rfc3339(),
        version: row.version,
    })
}

async fn load_payslips_for_run(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    run_id: Uuid,
    with_lines: bool,
    request_id: &str,
) -> Result<Vec<PayslipDto>, AppError> {
    let rows: Vec<PayslipRow> = sqlx::query_as(&format!(
        r#"
        SELECT {PAYSLIP_COLS}
        FROM people_payslip p
        JOIN people_payroll_run r ON r.id = p.run_id AND r.org_id = p.org_id
        JOIN people_employee e ON e.id = p.employee_id AND e.org_id = p.org_id
        WHERE p.org_id = $1 AND p.run_id = $2 AND p.deleted_at IS NULL
        ORDER BY e.display_name
        "#
    ))
    .bind(org_id)
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(
            load_payslip_dto(tx, org_id, row.id, with_lines, request_id)
                .await?,
        );
    }
    Ok(items)
}

fn parse_date(s: &str, request_id: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| validation(request_id, format!("invalid date: {s}")))
}
