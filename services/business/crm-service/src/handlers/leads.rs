//! `/api/v1/sales/leads` — lead capture, qualification, and conversion.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::Serialize;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{conflict, if_match_version, internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::dupes::find_lead_duplicates;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope, required_scope_for_owner_row};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::seed;
use crate::state::AppState;
use crate::types::{
    ConvertLeadRequest, ConvertLeadResponse, CreateLeadRequest, CustomerDto, DealDto,
    DisqualifyLeadRequest, DuplicateMatch, LeadDto, LeadListResponse, ListQuery, UpdateLeadRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sales/leads", get(list_leads).post(create_lead))
        .route(
            "/api/v1/sales/leads/{id}",
            get(get_lead).patch(update_lead),
        )
        .route("/api/v1/sales/leads/{id}/qualify", post(qualify_lead))
        .route("/api/v1/sales/leads/{id}/disqualify", post(disqualify_lead))
        .route("/api/v1/sales/leads/{id}/convert", post(convert_lead))
}

#[derive(Debug, sqlx::FromRow)]
struct LeadRow {
    public_id: String,
    name: String,
    email: Option<String>,
    phone: Option<String>,
    company_name: Option<String>,
    source: Option<String>,
    status: String,
    score: i32,
    owner_user_id: Option<Uuid>,
    notes: Option<String>,
    converted_customer_id: Option<Uuid>,
    converted_deal_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl LeadRow {
    fn into_dto(self) -> LeadDto {
        LeadDto {
            id: self.public_id,
            name: self.name,
            email: self.email,
            phone: self.phone,
            company_name: self.company_name,
            source: self.source,
            status: self.status,
            score: self.score,
            owner_user_id: self.owner_user_id.map(|u| PublicId::new(IdKind::User, u).as_str()),
            notes: self.notes,
            converted_customer_id: self
                .converted_customer_id
                .map(|u| PublicId::new(IdKind::Customer, u).as_str()),
            converted_deal_id: self
                .converted_deal_id
                .map(|u| PublicId::new(IdKind::Deal, u).as_str()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

const LEAD_COLUMNS: &str = "public_id, name, email, phone, company_name, source, status, score, owner_user_id, notes, converted_customer_id, converted_deal_id, created_at, updated_at, version";

/// GET /api/v1/sales/leads
#[utoipa::path(get, path = "/api/v1/sales/leads", tag = "sales-leads",
    responses((status = 200, body = LeadListResponse)))]
pub async fn list_leads(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<LeadListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(&membership.principal, perms::sales_lead_read(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_lead_read());
    let (limit, offset) = super::normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let build_filters = |qb: &mut QueryBuilder<Postgres>| {
        push_owner_predicate(qb, scope, org_id, actor, membership.team_id, membership.department_id);
        if let Some(term) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let pattern = format!("%{term}%");
            qb.push(" AND (name ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR email ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR company_name ILIKE ");
            qb.push_bind(pattern);
            qb.push(")");
        }
    };

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM sales_lead WHERE org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND deleted_at IS NULL");
    build_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new(format!("SELECT {LEAD_COLUMNS} FROM sales_lead WHERE org_id = "));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    build_filters(&mut qb);
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<LeadRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(LeadListResponse {
        items: rows.into_iter().map(LeadRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/sales/leads
#[utoipa::path(post, path = "/api/v1/sales/leads", tag = "sales-leads",
    request_body = CreateLeadRequest,
    responses((status = 201, body = LeadDto)))]
pub async fn create_lead(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateLeadRequest>,
) -> Result<(StatusCode, Json<LeadDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_lead_create(), &request_id)?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name must not be empty"));
    }
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => parse_public_id(IdKind::User, s, &request_id)?,
        None => auth.ctx.actor.user_id,
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Lead, id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO sales_lead (id, org_id, public_id, name, email, phone, company_name, source, owner_user_id, notes)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&body.name)
    .bind(&body.email)
    .bind(&body.phone)
    .bind(&body.company_name)
    .bind(&body.source)
    .bind(owner_user_id)
    .bind(&body.notes)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "lead",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": public_id.as_str(), "name": body.name }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit().await.map_err(internal(&request_id))?;

    Ok((
        StatusCode::CREATED,
        Json(LeadDto {
            id: public_id.as_str(),
            name: body.name,
            email: body.email,
            phone: body.phone,
            company_name: body.company_name,
            source: body.source,
            status: "new".into(),
            score: 0,
            owner_user_id: Some(PublicId::new(IdKind::User, owner_user_id).as_str()),
            notes: body.notes,
            converted_customer_id: None,
            converted_deal_id: None,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            version: 1,
        }),
    ))
}

async fn fetch_lead(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    lead_id: Uuid,
) -> Result<Option<LeadRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {LEAD_COLUMNS} FROM sales_lead WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(lead_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn enforce_lead_scope(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    auth: &AuthCtx,
    membership: &crate::principal::MembershipScope,
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

/// GET /api/v1/sales/leads/{id}
#[utoipa::path(get, path = "/api/v1/sales/leads/{id}", tag = "sales-leads",
    responses((status = 200, body = LeadDto), (status = 404)))]
pub async fn get_lead(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<LeadDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let lead_id = parse_public_id(IdKind::Lead, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_lead_read(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_lead(&mut tx, org_id, lead_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "lead"))?;
    enforce_lead_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_lead_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/sales/leads/{id}
#[utoipa::path(patch, path = "/api/v1/sales/leads/{id}", tag = "sales-leads",
    request_body = UpdateLeadRequest,
    responses((status = 200, body = LeadDto), (status = 404), (status = 409)))]
pub async fn update_lead(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateLeadRequest>,
) -> Result<Json<LeadDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let lead_id = parse_public_id(IdKind::Lead, &id, &request_id)?;
    let expected_version = if_match_version(&headers);

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_lead_update(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_lead(&mut tx, org_id, lead_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "lead"))?;
    enforce_lead_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_lead_update(),
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

    let name = body.name.unwrap_or(row.name);
    let email = body.email.or(row.email);
    let phone = body.phone.or(row.phone);
    let company_name = body.company_name.or(row.company_name);
    let source = body.source.or(row.source);
    let status = body.status.unwrap_or(row.status);
    let score = body.score.unwrap_or(row.score);
    let notes = body.notes.or(row.notes);
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => row.owner_user_id,
    };

    let updated: LeadRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_lead
        SET name = $3, email = $4, phone = $5, company_name = $6, source = $7, status = $8,
            score = $9, owner_user_id = $10, notes = $11, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {LEAD_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(lead_id)
    .bind(&name)
    .bind(&email)
    .bind(&phone)
    .bind(&company_name)
    .bind(&source)
    .bind(&status)
    .bind(score)
    .bind(owner_user_id)
    .bind(&notes)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}

async fn transition_status(
    state: &AppState,
    auth: &AuthCtx,
    lead_id_str: &str,
    new_status: &str,
    action: &str,
) -> Result<Json<LeadDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let lead_id = parse_public_id(IdKind::Lead, lead_id_str, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_lead_update(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_lead(&mut tx, org_id, lead_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "lead"))?;
    enforce_lead_scope(
        &mut tx,
        org_id,
        auth,
        &membership,
        perms::sales_lead_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == "converted" {
        return Err(conflict(&request_id, "lead already converted"));
    }

    let updated: LeadRow = sqlx::query_as(&format!(
        "UPDATE sales_lead SET status = $3, version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2 RETURNING {LEAD_COLUMNS}"
    ))
    .bind(org_id)
    .bind(lead_id)
    .bind(new_status)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        action,
        "lead",
        &updated.public_id,
        serde_json::json!({ "status": new_status }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}

/// POST /api/v1/sales/leads/{id}/qualify
#[utoipa::path(post, path = "/api/v1/sales/leads/{id}/qualify", tag = "sales-leads",
    responses((status = 200, body = LeadDto)))]
pub async fn qualify_lead(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<LeadDto>, AppError> {
    transition_status(&state, &auth, &id, "qualified", "sales.lead.qualify").await
}

/// POST /api/v1/sales/leads/{id}/disqualify
#[utoipa::path(post, path = "/api/v1/sales/leads/{id}/disqualify", tag = "sales-leads",
    request_body = DisqualifyLeadRequest,
    responses((status = 200, body = LeadDto)))]
pub async fn disqualify_lead(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(_body): Json<DisqualifyLeadRequest>,
) -> Result<Json<LeadDto>, AppError> {
    transition_status(&state, &auth, &id, "disqualified", "sales.lead.disqualify").await
}

#[derive(Debug, Serialize)]
struct ConflictWithMatches {
    #[serde(rename = "type")]
    type_uri: String,
    title: String,
    status: u16,
    detail: String,
    code: String,
    request_id: String,
    matches: Vec<DuplicateMatch>,
}

fn duplicates_response(request_id: &str, matches: Vec<DuplicateMatch>) -> Response {
    let body = ConflictWithMatches {
        type_uri: "https://companyos.dev/problems/conflict".into(),
        title: "Conflict".into(),
        status: 409,
        detail: format!("found {} potential duplicate customer(s)", matches.len()),
        code: "conflict".into(),
        request_id: request_id.to_string(),
        matches,
    };
    let mut res = (StatusCode::CONFLICT, Json(body)).into_response();
    res.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/problem+json"),
    );
    res
}

/// POST /api/v1/sales/leads/{id}/convert
///
/// One transaction: creates the customer + primary contact + deal, marks the
/// lead converted, and emits `lead.converted.v1` + `customer.created.v1`.
/// Duplicate detection runs before conversion — callers must pass
/// `force: true` to proceed despite matches. Idempotent on `Idempotency-Key`.
#[utoipa::path(post, path = "/api/v1/sales/leads/{id}/convert", tag = "sales-leads",
    request_body = ConvertLeadRequest,
    responses((status = 201, body = ConvertLeadResponse), (status = 409, description = "duplicates found or already converted")))]
pub async fn convert_lead(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ConvertLeadRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let lead_id = parse_public_id(IdKind::Lead, &id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_lead_convert(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, body)) = idempotency::get(&mut *tx, org_id, "lead.convert", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(body)).into_response());
        }
    }

    let row = fetch_lead(&mut tx, org_id, lead_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "lead"))?;
    enforce_lead_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_lead_convert(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == "converted" {
        // Idempotent-by-state: return the existing conversion result.
        if let (Some(cust_id), Some(deal_id)) = (row.converted_customer_id, row.converted_deal_id) {
            let customer = fetch_customer_dto(&mut tx, org_id, cust_id)
                .await
                .map_err(internal(&request_id))?
                .ok_or_else(|| not_found(&request_id, "converted customer"))?;
            let deal = fetch_deal_dto(&mut tx, org_id, deal_id)
                .await
                .map_err(internal(&request_id))?
                .ok_or_else(|| not_found(&request_id, "converted deal"))?;
            tx.commit().await.map_err(internal(&request_id))?;
            return Ok(Json(ConvertLeadResponse {
                lead: row.into_dto(),
                customer,
                deal,
            })
            .into_response());
        }
        return Err(conflict(&request_id, "lead already converted"));
    }

    let customer_name = row
        .company_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| row.name.clone());

    if !body.force {
        let duplicates = find_lead_duplicates(&mut tx, org_id, &customer_name, row.email.as_deref())
            .await
            .map_err(internal(&request_id))?
            .into_iter()
            .filter(|m| m.lead_id.as_deref() != Some(row.public_id.as_str()))
            .chain(
                crate::dupes::find_customer_duplicates(
                    &mut tx,
                    org_id,
                    &customer_name,
                    row.email.as_deref(),
                )
                .await
                .map_err(internal(&request_id))?,
            )
            .collect::<Vec<_>>();
        if !duplicates.is_empty() {
            return Ok(duplicates_response(&request_id, duplicates));
        }
    }

    let owner_user_id = row.owner_user_id.unwrap_or(auth.ctx.actor.user_id);

    let customer_id = new_uuid_v7();
    let customer_public = PublicId::new(IdKind::Customer, customer_id);
    sqlx::query(
        r#"
        INSERT INTO sales_customer (id, org_id, public_id, name, email, phone, owner_user_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(customer_id)
    .bind(org_id)
    .bind(customer_public.as_str())
    .bind(&customer_name)
    .bind(&row.email)
    .bind(&row.phone)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let contact_id = new_uuid_v7();
    let contact_public = PublicId::new(IdKind::Contact, contact_id);
    sqlx::query(
        r#"
        INSERT INTO sales_contact (id, org_id, public_id, customer_id, first_name, last_name, email, phone, is_primary, owner_user_id)
        VALUES ($1,$2,$3,$4,$5,'',$6,$7,true,$8)
        "#,
    )
    .bind(contact_id)
    .bind(org_id)
    .bind(contact_public.as_str())
    .bind(customer_id)
    .bind(&row.name)
    .bind(&row.email)
    .bind(&row.phone)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let pipeline_id = seed::ensure_default_pipeline(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;
    let stage_id = seed::default_open_stage(&mut tx, org_id, pipeline_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| AppError::new(ErrorCode::Internal, request_id.clone(), "no pipeline stage available"))?;

    let deal_id = new_uuid_v7();
    let deal_public = PublicId::new(IdKind::Deal, deal_id);
    let deal_name = body
        .deal_name
        .clone()
        .unwrap_or_else(|| format!("{customer_name} opportunity"));
    let amount_minor = body.amount_minor.unwrap_or(0);
    let currency = body.currency.clone().unwrap_or_else(|| "USD".into());

    sqlx::query(
        r#"
        INSERT INTO sales_deal (id, org_id, public_id, pipeline_id, stage_id, customer_id, lead_id, name, amount_minor, currency, owner_user_id, status)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'open')
        "#,
    )
    .bind(deal_id)
    .bind(org_id)
    .bind(deal_public.as_str())
    .bind(pipeline_id)
    .bind(stage_id)
    .bind(customer_id)
    .bind(lead_id)
    .bind(&deal_name)
    .bind(amount_minor)
    .bind(&currency)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let lead_updated: LeadRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_lead
        SET status = 'converted', converted_customer_id = $3, converted_deal_id = $4,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {LEAD_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(lead_id)
    .bind(customer_id)
    .bind(deal_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let customer_event = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "customer",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": customer_public.as_str(), "name": customer_name, "source": "lead_conversion" }),
    );
    companyos_outbox::insert_event(&mut *tx, &customer_event)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let convert_event = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "lead",
        "converted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": lead_updated.public_id,
            "customer_id": customer_public.as_str(),
            "deal_id": deal_public.as_str(),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &convert_event)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "sales.lead.convert",
        "lead",
        &lead_updated.public_id,
        serde_json::json!({ "customer_id": customer_public.as_str(), "deal_id": deal_public.as_str() }),
    )
    .await
    .map_err(internal(&request_id))?;

    let deal_dto = fetch_deal_dto(&mut tx, org_id, deal_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| AppError::new(ErrorCode::Internal, request_id.clone(), "deal missing after insert"))?;
    let customer_dto = fetch_customer_dto(&mut tx, org_id, customer_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| AppError::new(ErrorCode::Internal, request_id.clone(), "customer missing after insert"))?;

    let response = ConvertLeadResponse {
        lead: lead_updated.into_dto(),
        customer: customer_dto,
        deal: deal_dto,
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "lead.convert",
            key,
            201,
            serde_json::to_value(&response).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

async fn fetch_customer_dto(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    customer_id: Uuid,
) -> Result<Option<CustomerDto>, sqlx::Error> {
    #[derive(sqlx::FromRow)]
    struct Row {
        public_id: String,
        name: String,
        email: Option<String>,
        phone: Option<String>,
        website: Option<String>,
        billing_address: Option<String>,
        notes: Option<String>,
        owner_user_id: Option<Uuid>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        version: i32,
    }
    let row: Option<Row> = sqlx::query_as(
        r#"
        SELECT public_id, name, email, phone, website, billing_address, notes, owner_user_id, created_at, updated_at, version
        FROM sales_customer WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(customer_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|r| CustomerDto {
        id: r.public_id,
        name: r.name,
        email: r.email,
        phone: r.phone,
        website: r.website,
        billing_address: r.billing_address,
        notes: r.notes,
        owner_user_id: r.owner_user_id.map(|u| PublicId::new(IdKind::User, u).as_str()),
        created_at: r.created_at.to_rfc3339(),
        updated_at: r.updated_at.to_rfc3339(),
        version: r.version,
    }))
}

async fn fetch_deal_dto(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    deal_id: Uuid,
) -> Result<Option<DealDto>, sqlx::Error> {
    super::deals::fetch_deal_dto_by_id(tx, org_id, deal_id).await
}
