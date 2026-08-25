//! `/api/v1/sales/deals` — opportunities, stage moves, win/lose.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{conflict, if_match_version, internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope, required_scope_for_owner_row, MembershipScope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::seed;
use crate::state::AppState;
use crate::types::{
    CreateDealRequest, DealDto, DealListQuery, DealListResponse, LoseDealRequest, UpdateDealRequest,
    WinDealRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sales/deals", get(list_deals).post(create_deal))
        .route("/api/v1/sales/deals/board", get(super::pipelines::default_board))
        .route(
            "/api/v1/sales/deals/{id}",
            get(get_deal).patch(update_deal),
        )
        .route("/api/v1/sales/deals/{id}/win", post(win_deal))
        .route("/api/v1/sales/deals/{id}/lose", post(lose_deal))
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct DealRow {
    #[allow(dead_code)] // kept for symmetry with other *Row structs / future internal joins
    pub(crate) id: Uuid,
    pub(crate) public_id: String,
    pub(crate) pipeline_id: Uuid,
    pub(crate) stage_id: Uuid,
    pub(crate) customer_id: Option<Uuid>,
    pub(crate) lead_id: Option<Uuid>,
    pub(crate) name: String,
    pub(crate) amount_minor: i64,
    pub(crate) currency: String,
    pub(crate) probability: Option<i32>,
    pub(crate) expected_close_date: Option<NaiveDate>,
    pub(crate) owner_user_id: Option<Uuid>,
    pub(crate) status: String,
    pub(crate) won_reason: Option<String>,
    pub(crate) lost_reason: Option<String>,
    pub(crate) won_at: Option<DateTime<Utc>>,
    pub(crate) lost_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) version: i32,
}

pub(crate) const DEAL_COLUMNS: &str = "id, public_id, pipeline_id, stage_id, customer_id, lead_id, name, amount_minor, currency, probability, expected_close_date, owner_user_id, status, won_reason, lost_reason, won_at, lost_at, created_at, updated_at, version";

impl DealRow {
    pub(crate) fn into_dto(self) -> DealDto {
        DealDto {
            id: self.public_id,
            pipeline_id: PublicId::new(IdKind::Pipeline, self.pipeline_id).as_str(),
            stage_id: PublicId::new(IdKind::Stage, self.stage_id).as_str(),
            customer_id: self.customer_id.map(|u| PublicId::new(IdKind::Customer, u).as_str()),
            lead_id: self.lead_id.map(|u| PublicId::new(IdKind::Lead, u).as_str()),
            name: self.name,
            amount_minor: self.amount_minor,
            currency: self.currency,
            probability: self.probability,
            expected_close_date: self.expected_close_date.map(|d| d.to_string()),
            owner_user_id: self.owner_user_id.map(|u| PublicId::new(IdKind::User, u).as_str()),
            status: self.status,
            won_reason: self.won_reason,
            lost_reason: self.lost_reason,
            won_at: self.won_at.map(|t| t.to_rfc3339()),
            lost_at: self.lost_at.map(|t| t.to_rfc3339()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

pub(crate) async fn fetch_deal_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    deal_id: Uuid,
) -> Result<Option<DealRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {DEAL_COLUMNS} FROM sales_deal WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(deal_id)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn fetch_deal_dto_by_id(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    deal_id: Uuid,
) -> Result<Option<DealDto>, sqlx::Error> {
    Ok(fetch_deal_row(tx, org_id, deal_id).await?.map(DealRow::into_dto))
}

/// GET /api/v1/sales/deals
#[utoipa::path(get, path = "/api/v1/sales/deals", tag = "sales-deals",
    responses((status = 200, body = DealListResponse)))]
pub async fn list_deals(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<DealListQuery>,
) -> Result<Json<DealListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(&membership.principal, perms::sales_deal_read(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_deal_read());
    let (limit, offset) = super::normalize_paging(q.limit, q.offset);

    let stage_id = match q.stage_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::Stage, s, &request_id)?),
        None => None,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let build_filters = |qb: &mut QueryBuilder<Postgres>| {
        push_owner_predicate(qb, scope, org_id, actor, membership.team_id, membership.department_id);
        if let Some(term) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            qb.push(" AND name ILIKE ");
            qb.push_bind(format!("%{term}%"));
        }
        if let Some(stage) = stage_id {
            qb.push(" AND stage_id = ");
            qb.push_bind(stage);
        }
        if let Some(status) = q.status.as_deref().filter(|s| !s.is_empty()) {
            qb.push(" AND status = ");
            qb.push_bind(status.to_string());
        }
    };

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM sales_deal WHERE org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND deleted_at IS NULL");
    build_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new(format!("SELECT {DEAL_COLUMNS} FROM sales_deal WHERE org_id = "));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    build_filters(&mut qb);
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<DealRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(DealListResponse {
        items: rows.into_iter().map(DealRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/sales/deals
#[utoipa::path(post, path = "/api/v1/sales/deals", tag = "sales-deals",
    request_body = CreateDealRequest,
    responses((status = 201, body = DealDto)))]
pub async fn create_deal(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateDealRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_deal_create(), &request_id)?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name must not be empty"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "deal.create", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let customer_id = super::parse_optional_public_id(IdKind::Customer, body.customer_id.as_deref(), &request_id)?;
    let lead_id = super::parse_optional_public_id(IdKind::Lead, body.lead_id.as_deref(), &request_id)?;
    let pipeline_id = match super::parse_optional_public_id(IdKind::Pipeline, body.pipeline_id.as_deref(), &request_id)? {
        Some(p) => p,
        None => seed::ensure_default_pipeline(&mut tx, org_id)
            .await
            .map_err(internal(&request_id))?,
    };
    let stage_id = match super::parse_optional_public_id(IdKind::Stage, body.stage_id.as_deref(), &request_id)? {
        Some(s) => s,
        None => seed::default_open_stage(&mut tx, org_id, pipeline_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| AppError::new(ErrorCode::Internal, request_id.clone(), "no pipeline stage available"))?,
    };
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => parse_public_id(IdKind::User, s, &request_id)?,
        None => auth.ctx.actor.user_id,
    };
    let currency = body.currency.clone().unwrap_or_else(|| "USD".into());
    let expected_close_date = body
        .expected_close_date
        .as_deref()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| validation(&request_id, "expected_close_date must be YYYY-MM-DD"))?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Deal, id);

    sqlx::query(
        r#"
        INSERT INTO sales_deal (
            id, org_id, public_id, pipeline_id, stage_id, customer_id, lead_id, name,
            amount_minor, currency, probability, expected_close_date, owner_user_id, status
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'open')
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(pipeline_id)
    .bind(stage_id)
    .bind(customer_id)
    .bind(lead_id)
    .bind(&body.name)
    .bind(body.amount_minor)
    .bind(&currency)
    .bind(body.probability)
    .bind(expected_close_date)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "deal",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": public_id.as_str(), "name": body.name }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let dto = fetch_deal_dto_by_id(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| AppError::new(ErrorCode::Internal, request_id.clone(), "deal missing after insert"))?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "deal.create",
            key,
            201,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

async fn enforce_deal_scope(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    auth: &AuthCtx,
    membership: &MembershipScope,
    permission: companyos_authz::PermissionId,
    owner_user_id: Option<Uuid>,
    request_id: &str,
) -> Result<(), AppError> {
    let required_scope = required_scope_for_owner_row(
        tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        owner_user_id,
    )
    .await
    .map_err(internal(request_id))?;
    crate::principal::enforce_scoped(&membership.principal, permission, required_scope, request_id)
}

/// GET /api/v1/sales/deals/{id}
#[utoipa::path(get, path = "/api/v1/sales/deals/{id}", tag = "sales-deals",
    responses((status = 200, body = DealDto), (status = 404)))]
pub async fn get_deal(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<DealDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let deal_id = parse_public_id(IdKind::Deal, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_deal_read(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_deal_row(&mut tx, org_id, deal_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "deal"))?;
    enforce_deal_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_deal_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/sales/deals/{id} — field + stage updates; writes stage history on stage change.
#[utoipa::path(patch, path = "/api/v1/sales/deals/{id}", tag = "sales-deals",
    request_body = UpdateDealRequest,
    responses((status = 200, body = DealDto), (status = 404), (status = 409)))]
pub async fn update_deal(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateDealRequest>,
) -> Result<Json<DealDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let deal_id = parse_public_id(IdKind::Deal, &id, &request_id)?;
    let expected_version = if_match_version(&headers);

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_deal_update(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_deal_row(&mut tx, org_id, deal_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "deal"))?;
    enforce_deal_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_deal_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if let Some(expected) = expected_version {
        if expected != row.version {
            return Err(conflict(
                &request_id,
                format!("version mismatch: expected {expected}, current {}", row.version),
            ));
        }
    }

    let new_stage_id = match body.stage_id.as_deref() {
        Some(s) => parse_public_id(IdKind::Stage, s, &request_id)?,
        None => row.stage_id,
    };
    let name = body.name.unwrap_or(row.name);
    let amount_minor = body.amount_minor.unwrap_or(row.amount_minor);
    let currency = body.currency.unwrap_or(row.currency);
    let probability = body.probability.or(row.probability);
    let expected_close_date = match body.expected_close_date.as_deref() {
        Some(s) => Some(
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| validation(&request_id, "expected_close_date must be YYYY-MM-DD"))?,
        ),
        None => row.expected_close_date,
    };
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => row.owner_user_id,
    };

    let updated: DealRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_deal
        SET stage_id = $3, name = $4, amount_minor = $5, currency = $6, probability = $7,
            expected_close_date = $8, owner_user_id = $9, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {DEAL_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(deal_id)
    .bind(new_stage_id)
    .bind(&name)
    .bind(amount_minor)
    .bind(&currency)
    .bind(probability)
    .bind(expected_close_date)
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if new_stage_id != row.stage_id {
        let hist_id = new_uuid_v7();
        sqlx::query(
            r#"
            INSERT INTO sales_deal_stage_history (id, org_id, public_id, deal_id, from_stage_id, to_stage_id, changed_by, note)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
            "#,
        )
        .bind(hist_id)
        .bind(org_id)
        .bind(format!("dsh_{hist_id}"))
        .bind(deal_id)
        .bind(row.stage_id)
        .bind(new_stage_id)
        .bind(auth.ctx.actor.user_id)
        .bind(&body.note)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "sales.deal.update",
        "deal",
        &updated.public_id,
        serde_json::json!({ "stage_id": PublicId::new(IdKind::Stage, new_stage_id).as_str() }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}

/// POST /api/v1/sales/deals/{id}/win
///
/// **Idempotent**: if the deal is already `won`, returns the current deal
/// without emitting a second `deal.won.v1` event.
#[utoipa::path(post, path = "/api/v1/sales/deals/{id}/win", tag = "sales-deals",
    request_body = WinDealRequest,
    responses((status = 200, body = DealDto)))]
pub async fn win_deal(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<WinDealRequest>,
) -> Result<Json<DealDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let deal_id = parse_public_id(IdKind::Deal, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_deal_win(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_deal_row(&mut tx, org_id, deal_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "deal"))?;
    enforce_deal_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_deal_win(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == "won" {
        // Idempotent: no second event, no state change.
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(row.into_dto()));
    }

    let won_stage: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM sales_pipeline_stage WHERE org_id = $1 AND pipeline_id = $2 AND is_won AND deleted_at IS NULL ORDER BY position ASC LIMIT 1",
    )
    .bind(org_id)
    .bind(row.pipeline_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let target_stage = won_stage.map(|(s,)| s).unwrap_or(row.stage_id);

    let updated: DealRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_deal
        SET status = 'won', stage_id = $3, won_reason = $4, won_at = now(),
            probability = 100, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {DEAL_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(deal_id)
    .bind(target_stage)
    .bind(&body.reason)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if target_stage != row.stage_id {
        let hist_id = new_uuid_v7();
        sqlx::query(
            r#"
            INSERT INTO sales_deal_stage_history (id, org_id, public_id, deal_id, from_stage_id, to_stage_id, changed_by, note)
            VALUES ($1,$2,$3,$4,$5,$6,$7,'won')
            "#,
        )
        .bind(hist_id)
        .bind(org_id)
        .bind(format!("dsh_{hist_id}"))
        .bind(deal_id)
        .bind(row.stage_id)
        .bind(target_stage)
        .bind(auth.ctx.actor.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "deal",
        "won",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": updated.public_id, "amount_minor": updated.amount_minor, "currency": updated.currency }),
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
        "sales.deal.win",
        "deal",
        &updated.public_id,
        serde_json::json!({ "reason": body.reason }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}

/// POST /api/v1/sales/deals/{id}/lose
#[utoipa::path(post, path = "/api/v1/sales/deals/{id}/lose", tag = "sales-deals",
    request_body = LoseDealRequest,
    responses((status = 200, body = DealDto)))]
pub async fn lose_deal(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<LoseDealRequest>,
) -> Result<Json<DealDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let deal_id = parse_public_id(IdKind::Deal, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_deal_lose(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_deal_row(&mut tx, org_id, deal_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "deal"))?;
    enforce_deal_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_deal_lose(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == "lost" {
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(row.into_dto()));
    }

    let lost_stage: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM sales_pipeline_stage WHERE org_id = $1 AND pipeline_id = $2 AND is_lost AND deleted_at IS NULL ORDER BY position ASC LIMIT 1",
    )
    .bind(org_id)
    .bind(row.pipeline_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let target_stage = lost_stage.map(|(s,)| s).unwrap_or(row.stage_id);

    let updated: DealRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_deal
        SET status = 'lost', stage_id = $3, lost_reason = $4, lost_at = now(),
            probability = 0, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {DEAL_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(deal_id)
    .bind(target_stage)
    .bind(&body.reason)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if target_stage != row.stage_id {
        let hist_id = new_uuid_v7();
        sqlx::query(
            r#"
            INSERT INTO sales_deal_stage_history (id, org_id, public_id, deal_id, from_stage_id, to_stage_id, changed_by, note)
            VALUES ($1,$2,$3,$4,$5,$6,$7,'lost')
            "#,
        )
        .bind(hist_id)
        .bind(org_id)
        .bind(format!("dsh_{hist_id}"))
        .bind(deal_id)
        .bind(row.stage_id)
        .bind(target_stage)
        .bind(auth.ctx.actor.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "deal",
        "lost",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": updated.public_id, "reason": body.reason }),
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
        "sales.deal.lose",
        "deal",
        &updated.public_id,
        serde_json::json!({ "reason": body.reason }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}
