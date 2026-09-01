//! `/api/v1/sales/contracts` — customer contracts, publish, and renewal pipeline.

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
use crate::principal::{
    enforce_any_scope, load_membership_scope_for, required_scope_for_owner_row, MembershipScope,
};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    ContractDto, ContractListQuery, ContractListResponse, CreateContractRequest, RenewalListQuery,
    RenewalPipelineResponse, UpdateContractRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/sales/contracts",
            get(list_contracts).post(create_contract),
        )
        .route("/api/v1/sales/contracts/renewals", get(list_renewals))
        .route(
            "/api/v1/sales/contracts/{id}",
            get(get_contract).patch(update_contract),
        )
        .route(
            "/api/v1/sales/contracts/{id}/publish",
            post(publish_contract),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ContractRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    customer_id: Uuid,
    deal_id: Option<Uuid>,
    order_id: Option<Uuid>,
    title: String,
    status: String,
    term_months: Option<i32>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    value_minor: i64,
    currency: String,
    auto_renew: bool,
    renewal_notice_days: i32,
    owner_user_id: Option<Uuid>,
    published_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const CONTRACT_COLUMNS: &str = "id, public_id, customer_id, deal_id, order_id, title, status, term_months, start_date, end_date, value_minor, currency, auto_renew, renewal_notice_days, owner_user_id, published_at, created_at, updated_at, version";

impl ContractRow {
    fn into_dto(self) -> ContractDto {
        ContractDto {
            id: self.public_id,
            customer_id: PublicId::new(IdKind::Customer, self.customer_id).as_str(),
            deal_id: self
                .deal_id
                .map(|u| PublicId::new(IdKind::Deal, u).as_str()),
            order_id: self
                .order_id
                .map(|u| PublicId::new(IdKind::SalesOrder, u).as_str()),
            title: self.title,
            status: self.status,
            term_months: self.term_months,
            start_date: self.start_date.map(|d| d.to_string()),
            end_date: self.end_date.map(|d| d.to_string()),
            value_minor: self.value_minor,
            currency: self.currency,
            auto_renew: self.auto_renew,
            renewal_notice_days: self.renewal_notice_days,
            owner_user_id: self
                .owner_user_id
                .map(|u| PublicId::new(IdKind::User, u).as_str()),
            published_at: self.published_at.map(|t| t.to_rfc3339()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

async fn fetch_contract_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    contract_id: Uuid,
) -> Result<Option<ContractRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {CONTRACT_COLUMNS} FROM sales_contract WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(contract_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn enforce_contract_scope(
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
    crate::principal::enforce_scoped(
        &membership.principal,
        permission,
        required_scope,
        request_id,
    )
}

fn parse_date(raw: &str, request_id: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| validation(request_id, "date must be YYYY-MM-DD"))
}

/// GET /api/v1/sales/contracts
#[utoipa::path(get, path = "/api/v1/sales/contracts", tag = "sales-contracts",
    responses((status = 200, body = ContractListResponse)))]
pub async fn list_contracts(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ContractListQuery>,
) -> Result<Json<ContractListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_contract_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_contract_read());
    let (limit, offset) = super::normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let build_filters = |qb: &mut QueryBuilder<Postgres>| {
        push_owner_predicate(
            qb,
            scope,
            org_id,
            actor,
            membership.team_id,
            membership.department_id,
        );
        if let Some(status) = q.status.as_deref().filter(|s| !s.is_empty()) {
            qb.push(" AND status = ");
            qb.push_bind(status.to_string());
        }
    };

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM sales_contract WHERE org_id = ");
    count_qb.push_bind(org_id);
    build_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {CONTRACT_COLUMNS} FROM sales_contract WHERE org_id = "
    ));
    qb.push_bind(org_id);
    build_filters(&mut qb);
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<ContractRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ContractListResponse {
        items: rows.into_iter().map(ContractRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/sales/contracts
#[utoipa::path(post, path = "/api/v1/sales/contracts", tag = "sales-contracts",
    request_body = CreateContractRequest,
    responses((status = 201, body = ContractDto)))]
pub async fn create_contract(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateContractRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_contract_create(),
        &request_id,
    )?;

    if body.title.trim().is_empty() {
        return Err(validation(&request_id, "title must not be empty"));
    }

    let currency = body.currency.clone().unwrap_or_else(|| "USD".into());
    companyos_money::Currency::new(&currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;

    let start_date = body
        .start_date
        .as_deref()
        .map(|s| parse_date(s, &request_id))
        .transpose()?;
    let end_date = body
        .end_date
        .as_deref()
        .map(|s| parse_date(s, &request_id))
        .transpose()?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "contract.create", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let customer_id = parse_public_id(IdKind::Customer, &body.customer_id, &request_id)?;
    let deal_id =
        super::parse_optional_public_id(IdKind::Deal, body.deal_id.as_deref(), &request_id)?;
    let order_id =
        super::parse_optional_public_id(IdKind::SalesOrder, body.order_id.as_deref(), &request_id)?;
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => parse_public_id(IdKind::User, s, &request_id)?,
        None => auth.ctx.actor.user_id,
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::SalesContract, id);

    let row: ContractRow = sqlx::query_as(&format!(
        r#"
        INSERT INTO sales_contract (
            id, org_id, public_id, customer_id, deal_id, order_id, title, status,
            term_months, start_date, end_date, value_minor, currency, auto_renew,
            renewal_notice_days, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,'draft',$8,$9,$10,$11,$12,$13,$14,$15)
        RETURNING {CONTRACT_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(customer_id)
    .bind(deal_id)
    .bind(order_id)
    .bind(&body.title)
    .bind(body.term_months)
    .bind(start_date)
    .bind(end_date)
    .bind(body.value_minor.unwrap_or(0))
    .bind(&currency)
    .bind(body.auto_renew)
    .bind(body.renewal_notice_days.unwrap_or(30))
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = row.into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "contract",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "customer_id": dto.customer_id,
            "title": dto.title,
            "status": dto.status,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "contract.create",
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

/// GET /api/v1/sales/contracts/{id}
#[utoipa::path(get, path = "/api/v1/sales/contracts/{id}", tag = "sales-contracts",
    responses((status = 200, body = ContractDto), (status = 404)))]
pub async fn get_contract(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ContractDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let contract_id = parse_public_id(IdKind::SalesContract, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_contract_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_contract_row(&mut tx, org_id, contract_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "contract"))?;
    enforce_contract_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_contract_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/sales/contracts/{id} — draft-only edits; published contracts 409.
#[utoipa::path(patch, path = "/api/v1/sales/contracts/{id}", tag = "sales-contracts",
    request_body = UpdateContractRequest,
    responses((status = 200, body = ContractDto), (status = 404), (status = 409)))]
pub async fn update_contract(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateContractRequest>,
) -> Result<Json<ContractDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let contract_id = parse_public_id(IdKind::SalesContract, &id, &request_id)?;
    let expected_version = if_match_version(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_contract_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_contract_row(&mut tx, org_id, contract_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "contract"))?;
    enforce_contract_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_contract_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.published_at.is_some() || row.status == "active" {
        return Err(conflict(
            &request_id,
            "published contract is immutable; cannot PATCH after publish",
        ));
    }

    if let Some(expected) = expected_version {
        if expected != row.version {
            return Err(conflict(
                &request_id,
                format!(
                    "version mismatch: expected {expected}, current {}",
                    row.version
                ),
            ));
        }
    }

    let title = body.title.unwrap_or(row.title);
    let term_months = body.term_months.or(row.term_months);
    let start_date = match body.start_date.as_deref() {
        Some(s) => Some(parse_date(s, &request_id)?),
        None => row.start_date,
    };
    let end_date = match body.end_date.as_deref() {
        Some(s) => Some(parse_date(s, &request_id)?),
        None => row.end_date,
    };
    let value_minor = body.value_minor.unwrap_or(row.value_minor);
    let currency = body.currency.unwrap_or(row.currency);
    let auto_renew = body.auto_renew.unwrap_or(row.auto_renew);
    let renewal_notice_days = body.renewal_notice_days.unwrap_or(row.renewal_notice_days);
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => row.owner_user_id,
    };
    let status = body.status.unwrap_or(row.status);
    if !matches!(status.as_str(), "draft" | "cancelled") {
        return Err(validation(
            &request_id,
            "status on draft contract must be draft|cancelled",
        ));
    }

    let updated: ContractRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_contract
        SET title = $3, term_months = $4, start_date = $5, end_date = $6,
            value_minor = $7, currency = $8, auto_renew = $9, renewal_notice_days = $10,
            owner_user_id = $11, status = $12, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {CONTRACT_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(contract_id)
    .bind(&title)
    .bind(term_months)
    .bind(start_date)
    .bind(end_date)
    .bind(value_minor)
    .bind(&currency)
    .bind(auto_renew)
    .bind(renewal_notice_days)
    .bind(owner_user_id)
    .bind(&status)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "sales.contract.update",
        "contract",
        &updated.public_id,
        serde_json::json!({ "title": title }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}

/// POST /api/v1/sales/contracts/{id}/publish
#[utoipa::path(post, path = "/api/v1/sales/contracts/{id}/publish", tag = "sales-contracts",
    responses((status = 200, body = ContractDto), (status = 403), (status = 409)))]
pub async fn publish_contract(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ContractDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let contract_id = parse_public_id(IdKind::SalesContract, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_contract_publish(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_contract_row(&mut tx, org_id, contract_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "contract"))?;
    enforce_contract_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_contract_publish(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == "active" && row.published_at.is_some() {
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(row.into_dto()));
    }

    if row.status != "draft" {
        return Err(conflict(
            &request_id,
            format!("cannot publish contract in status {}", row.status),
        ));
    }

    let updated: ContractRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_contract
        SET status = 'active', published_at = now(), version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {CONTRACT_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(contract_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = updated.clone().into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "contract",
        "published",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "customer_id": dto.customer_id,
            "title": dto.title,
            "end_date": dto.end_date,
            "auto_renew": dto.auto_renew,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    // Emit renewal_upcoming when end_date is within the default notice window (or auto_renew).
    let emit_renewal = dto.auto_renew
        || dto.end_date.as_ref().is_some_and(|end| {
            NaiveDate::parse_from_str(end, "%Y-%m-%d")
                .ok()
                .is_some_and(|d| {
                    let today = Utc::now().date_naive();
                    let days = (d - today).num_days();
                    days >= 0 && days <= i64::from(dto.renewal_notice_days.max(90))
                })
        });
    if emit_renewal {
        let renewal = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Sales,
            "contract",
            "renewal_upcoming",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "id": dto.id,
                "end_date": dto.end_date,
                "auto_renew": dto.auto_renew,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &renewal)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "sales.contract.publish",
        "contract",
        &dto.id,
        serde_json::json!({ "status": "active" }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// GET /api/v1/sales/contracts/renewals?within_days=90
#[utoipa::path(get, path = "/api/v1/sales/contracts/renewals", tag = "sales-contracts",
    responses((status = 200, body = RenewalPipelineResponse)))]
pub async fn list_renewals(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<RenewalListQuery>,
) -> Result<Json<RenewalPipelineResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;
    let within_days = q.within_days.unwrap_or(90).clamp(1, 365);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_contract_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_contract_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {CONTRACT_COLUMNS} FROM sales_contract WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND status = 'active'");
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    qb.push(" AND (auto_renew = true OR (end_date IS NOT NULL AND end_date <= (CURRENT_DATE + ");
    qb.push_bind(within_days);
    qb.push(" * INTERVAL '1 day') AND end_date >= CURRENT_DATE))");
    qb.push(" ORDER BY end_date ASC NULLS LAST");

    let rows: Vec<ContractRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let items: Vec<ContractDto> = rows.into_iter().map(ContractRow::into_dto).collect();

    // Signal upcoming renewals via outbox (one event summarizing the pipeline listing).
    if !items.is_empty() {
        let envelope = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Sales,
            "contract",
            "renewal_upcoming",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "within_days": within_days,
                "count": items.len(),
                "contract_ids": items.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &envelope)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(RenewalPipelineResponse { items, within_days }))
}
