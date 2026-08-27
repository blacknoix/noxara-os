//! `/api/v1/finance/periods` — fiscal period open / close / reopen / checklist.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{conflict, internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::periods::default_checklist;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    ClosePeriodRequest, CreateFiscalPeriodRequest, FiscalPeriodDto, FiscalPeriodListResponse,
    ReopenPeriodRequest, UpdateChecklistRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/periods",
            get(list_periods).post(create_period),
        )
        .route("/api/v1/finance/periods/{id}/close", post(close_period))
        .route("/api/v1/finance/periods/{id}/reopen", post(reopen_period))
        .route(
            "/api/v1/finance/periods/{id}/checklist",
            patch(update_checklist),
        )
}

#[derive(Debug, sqlx::FromRow)]
struct PeriodRow {
    public_id: String,
    code: String,
    name: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
    status: String,
    checklist: serde_json::Value,
    closed_at: Option<DateTime<Utc>>,
    reopened_at: Option<DateTime<Utc>>,
    reopen_reason: Option<String>,
}

impl PeriodRow {
    fn into_dto(self) -> FiscalPeriodDto {
        FiscalPeriodDto {
            id: self.public_id,
            code: self.code,
            name: self.name,
            start_date: self.start_date.to_string(),
            end_date: self.end_date.to_string(),
            status: self.status,
            checklist: self.checklist,
            closed_at: self.closed_at.map(|t| t.to_rfc3339()),
            reopened_at: self.reopened_at.map(|t| t.to_rfc3339()),
            reopen_reason: self.reopen_reason,
        }
    }
}

const PERIOD_SELECT: &str = r#"
    public_id, code, name, start_date, end_date, status, checklist,
    closed_at, reopened_at, reopen_reason
"#;

async fn fetch_period(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    period_id: Uuid,
) -> Result<Option<PeriodRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {PERIOD_SELECT} FROM finance_fiscal_period WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(period_id)
    .fetch_optional(&mut **tx)
    .await
}

fn parse_date(request_id: &str, raw: &str, field: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| validation(request_id, format!("{field} must be YYYY-MM-DD")))
}

/// GET /api/v1/finance/periods
#[utoipa::path(
    get,
    path = "/api/v1/finance/periods",
    tag = "finance-periods",
    responses((status = 200, body = FiscalPeriodListResponse))
)]
pub async fn list_periods(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<FiscalPeriodListResponse>, AppError> {
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
        perms::finance_period_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM finance_fiscal_period WHERE org_id = $1")
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal(&request_id))?;

    let rows: Vec<PeriodRow> = sqlx::query_as(&format!(
        "SELECT {PERIOD_SELECT} FROM finance_fiscal_period
         WHERE org_id = $1 ORDER BY start_date DESC"
    ))
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(FiscalPeriodListResponse {
        items: rows.into_iter().map(PeriodRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/finance/periods
#[utoipa::path(
    post,
    path = "/api/v1/finance/periods",
    tag = "finance-periods",
    request_body = CreateFiscalPeriodRequest,
    responses((status = 201, body = FiscalPeriodDto))
)]
pub async fn create_period(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateFiscalPeriodRequest>,
) -> Result<impl IntoResponse, AppError> {
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
        perms::finance_period_close(),
        &request_id,
    )?;

    let code = body.code.trim();
    if code.is_empty() {
        return Err(validation(&request_id, "code required"));
    }
    let name = body.name.trim();
    if name.is_empty() {
        return Err(validation(&request_id, "name required"));
    }
    let start = parse_date(&request_id, &body.start_date, "start_date")?;
    let end = parse_date(&request_id, &body.end_date, "end_date")?;
    if end < start {
        return Err(validation(&request_id, "end_date must be >= start_date"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::FiscalPeriod, id);
    let checklist = default_checklist();

    let result = sqlx::query(
        r#"
        INSERT INTO finance_fiscal_period (
            id, org_id, public_id, code, name, start_date, end_date, status, checklist
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,'open',$8)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(code)
    .bind(name)
    .bind(start)
    .bind(end)
    .bind(&checklist)
    .execute(&mut *tx)
    .await;

    if let Err(e) = result {
        if super::is_unique_violation(&e, "finance_fiscal_period_org_id_code_key") {
            return Err(conflict(
                &request_id,
                format!("period code {code} already exists"),
            ));
        }
        return Err(internal(&request_id)(e));
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.period.create",
        "fiscal_period",
        &public_id.as_str(),
        serde_json::json!({ "code": code }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_period(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "period missing after insert",
            )
        })?
        .into_dto();
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// POST /api/v1/finance/periods/{id}/close
#[utoipa::path(
    post,
    path = "/api/v1/finance/periods/{id}/close",
    tag = "finance-periods",
    request_body = ClosePeriodRequest,
    responses((status = 200, body = FiscalPeriodDto), (status = 201, body = FiscalPeriodDto))
)]
pub async fn close_period(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ClosePeriodRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let period_id = parse_public_id(IdKind::FiscalPeriod, &id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_period_close(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "period.close", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let row = fetch_period(&mut tx, org_id, period_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "fiscal period"))?;

    if row.status == "closed" || row.status == "locked" {
        let dto = row.into_dto();
        if let Some(key) = idem_key.as_deref() {
            idempotency::put(
                &mut *tx,
                org_id,
                "period.close",
                key,
                200,
                serde_json::to_value(&dto).unwrap_or_default(),
            )
            .await
            .map_err(internal(&request_id))?;
        }
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((StatusCode::OK, Json(dto)).into_response());
    }
    if row.status != "open" {
        return Err(conflict(
            &request_id,
            format!("period status {} cannot be closed", row.status),
        ));
    }

    if let Some(ref checklist) = body.checklist {
        sqlx::query(
            "UPDATE finance_fiscal_period SET checklist = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(period_id)
        .bind(checklist)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    sqlx::query(
        r#"
        UPDATE finance_fiscal_period SET
            status = 'closed',
            closed_at = now(),
            closed_by = $3,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(period_id)
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_period(&mut tx, org_id, period_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "fiscal period"))?
        .into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "period",
        "closed",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "code": dto.code }),
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
        "finance.period.close",
        "fiscal_period",
        &dto.id,
        serde_json::json!({ "code": dto.code }),
    )
    .await
    .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "period.close",
            key,
            200,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::OK, Json(dto)).into_response())
}

/// POST /api/v1/finance/periods/{id}/reopen
#[utoipa::path(
    post,
    path = "/api/v1/finance/periods/{id}/reopen",
    tag = "finance-periods",
    request_body = ReopenPeriodRequest,
    responses((status = 200, body = FiscalPeriodDto))
)]
pub async fn reopen_period(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<ReopenPeriodRequest>,
) -> Result<Json<FiscalPeriodDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let period_id = parse_public_id(IdKind::FiscalPeriod, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_period_reopen(),
        &request_id,
    )?;

    let reason = body.reason.trim();
    if reason.is_empty() {
        return Err(validation(&request_id, "reason required to reopen period"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_period(&mut tx, org_id, period_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "fiscal period"))?;

    if row.status == "open" {
        return Ok(Json(row.into_dto()));
    }
    if row.status != "closed" && row.status != "locked" {
        return Err(conflict(
            &request_id,
            format!("period status {} cannot be reopened", row.status),
        ));
    }

    sqlx::query(
        r#"
        UPDATE finance_fiscal_period SET
            status = 'open',
            reopened_at = now(),
            reopened_by = $3,
            reopen_reason = $4,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(period_id)
    .bind(auth.ctx.actor.user_id)
    .bind(reason)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_period(&mut tx, org_id, period_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "fiscal period"))?
        .into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "period",
        "reopened",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "code": dto.code,
            "reason": reason,
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
        "finance.period.reopen",
        "fiscal_period",
        &dto.id,
        serde_json::json!({ "reason": reason }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// PATCH /api/v1/finance/periods/{id}/checklist
#[utoipa::path(
    patch,
    path = "/api/v1/finance/periods/{id}/checklist",
    tag = "finance-periods",
    request_body = UpdateChecklistRequest,
    responses((status = 200, body = FiscalPeriodDto))
)]
pub async fn update_checklist(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateChecklistRequest>,
) -> Result<Json<FiscalPeriodDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let period_id = parse_public_id(IdKind::FiscalPeriod, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_period_close(),
        &request_id,
    )?;

    if !body.checklist.is_object() {
        return Err(validation(&request_id, "checklist must be a JSON object"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_period(&mut tx, org_id, period_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "fiscal period"))?;

    if row.status != "open" {
        return Err(conflict(
            &request_id,
            "checklist can only be updated on open periods",
        ));
    }

    sqlx::query(
        r#"
        UPDATE finance_fiscal_period SET
            checklist = $3,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(period_id)
    .bind(&body.checklist)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_period(&mut tx, org_id, period_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "fiscal period"))?
        .into_dto();
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
