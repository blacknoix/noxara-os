//! `/api/v1/sales/quotes` — quote drafting, versioning, and acceptance.
//!
//! Accepted quotes are immutable: [`update_quote`] on an `accepted` quote
//! creates a brand-new row (`version_number + 1`, `previous_quote_id` set,
//! `status = draft`) instead of mutating the accepted row in place.

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
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{conflict, internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{
    enforce_any_scope, load_membership_scope, required_scope_for_owner_row, MembershipScope,
};
use crate::quotes_math::{compute_quote_totals, LineInput};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreateQuoteLineRequest, CreateQuoteRequest, InvoiceActionResponse, ListQuery, QuoteDto,
    QuoteLineDto, QuoteListResponse, RejectQuoteRequest, UpdateQuoteRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sales/quotes", get(list_quotes).post(create_quote))
        .route(
            "/api/v1/sales/quotes/{id}",
            get(get_quote).patch(update_quote),
        )
        .route("/api/v1/sales/quotes/{id}/send", post(send_quote))
        .route("/api/v1/sales/quotes/{id}/accept", post(accept_quote))
        .route("/api/v1/sales/quotes/{id}/reject", post(reject_quote))
        .route(
            "/api/v1/sales/quotes/{id}/invoice-action",
            get(invoice_action),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct QuoteRow {
    id: Uuid,
    public_id: String,
    deal_id: Option<Uuid>,
    customer_id: Uuid,
    quote_number: String,
    status: String,
    version_number: i32,
    previous_quote_id: Option<Uuid>,
    currency: String,
    subtotal_minor: i64,
    discount_minor: i64,
    tax_minor: i64,
    total_minor: i64,
    notes: Option<String>,
    valid_until: Option<NaiveDate>,
    accepted_at: Option<DateTime<Utc>>,
    owner_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const QUOTE_COLUMNS: &str = "id, public_id, deal_id, customer_id, quote_number, status, version_number, previous_quote_id, currency, subtotal_minor, discount_minor, tax_minor, total_minor, notes, valid_until, accepted_at, owner_user_id, created_at, updated_at, version";

#[derive(Debug, Clone, sqlx::FromRow)]
struct QuoteLineRow {
    public_id: String,
    position: i32,
    product_id: Option<Uuid>,
    description: String,
    quantity: i32,
    unit_price_minor: i64,
    discount_minor: i64,
    tax_rate_bps: i32,
    tax_minor: i64,
    line_total_minor: i64,
}

const QUOTE_LINE_COLUMNS: &str = "public_id, position, product_id, description, quantity, unit_price_minor, discount_minor, tax_rate_bps, tax_minor, line_total_minor";

impl QuoteLineRow {
    fn into_dto(self) -> QuoteLineDto {
        QuoteLineDto {
            id: self.public_id,
            position: self.position,
            product_id: self
                .product_id
                .map(|u| PublicId::new(IdKind::Product, u).as_str()),
            description: self.description,
            quantity: self.quantity,
            unit_price_minor: self.unit_price_minor,
            discount_minor: self.discount_minor,
            tax_rate_bps: self.tax_rate_bps,
            tax_minor: self.tax_minor,
            line_total_minor: self.line_total_minor,
        }
    }
}

async fn fetch_lines(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    quote_id: Uuid,
) -> Result<Vec<QuoteLineRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {QUOTE_LINE_COLUMNS} FROM sales_quote_line WHERE org_id = $1 AND quote_id = $2 ORDER BY position ASC"
    ))
    .bind(org_id)
    .bind(quote_id)
    .fetch_all(&mut **tx)
    .await
}

fn assemble_dto(row: QuoteRow, lines: Vec<QuoteLineRow>) -> QuoteDto {
    QuoteDto {
        id: row.public_id,
        deal_id: row.deal_id.map(|u| PublicId::new(IdKind::Deal, u).as_str()),
        customer_id: PublicId::new(IdKind::Customer, row.customer_id).as_str(),
        quote_number: row.quote_number,
        status: row.status,
        version_number: row.version_number,
        previous_quote_id: row
            .previous_quote_id
            .map(|u| PublicId::new(IdKind::Quote, u).as_str()),
        currency: row.currency,
        subtotal_minor: row.subtotal_minor,
        discount_minor: row.discount_minor,
        tax_minor: row.tax_minor,
        total_minor: row.total_minor,
        notes: row.notes,
        valid_until: row.valid_until.map(|d| d.to_string()),
        accepted_at: row.accepted_at.map(|t| t.to_rfc3339()),
        owner_user_id: row
            .owner_user_id
            .map(|u| PublicId::new(IdKind::User, u).as_str()),
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        version: row.version,
        lines: lines.into_iter().map(QuoteLineRow::into_dto).collect(),
    }
}

async fn fetch_quote_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    quote_id: Uuid,
) -> Result<Option<QuoteRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {QUOTE_COLUMNS} FROM sales_quote WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(quote_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn fetch_quote_dto(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    quote_id: Uuid,
) -> Result<Option<QuoteDto>, sqlx::Error> {
    let Some(row) = fetch_quote_row(tx, org_id, quote_id).await? else {
        return Ok(None);
    };
    let lines = fetch_lines(tx, org_id, row.id).await?;
    Ok(Some(assemble_dto(row, lines)))
}

/// Compute totals for a set of line requests and validate currency.
fn compute_lines(
    lines: &[CreateQuoteLineRequest],
    currency: Currency,
    request_id: &str,
) -> Result<Vec<crate::quotes_math::LineTotals>, AppError> {
    let inputs: Vec<LineInput> = lines
        .iter()
        .map(|l| LineInput {
            quantity: l.quantity as i64,
            unit_price_minor: l.unit_price_minor,
            discount_minor: l.discount_minor,
            tax_rate_bps: l.tax_rate_bps as i64,
        })
        .collect();
    let (computed, _doc) = compute_quote_totals(&inputs, currency)
        .map_err(|e| validation(request_id, format!("invalid quote totals: {e}")))?;
    Ok(computed)
}

/// Validate/parse each line's optional `product_id`, surfacing bad ids as a
/// 400 rather than silently dropping them.
fn parse_line_product_ids(
    lines: &[CreateQuoteLineRequest],
    request_id: &str,
) -> Result<Vec<Option<Uuid>>, AppError> {
    lines
        .iter()
        .map(|l| {
            super::parse_optional_public_id(IdKind::Product, l.product_id.as_deref(), request_id)
        })
        .collect()
}

/// Insert quote lines for `quote_id`, returning the persisted rows.
async fn insert_lines(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    quote_id: Uuid,
    lines: &[CreateQuoteLineRequest],
    product_ids: &[Option<Uuid>],
    computed: &[crate::quotes_math::LineTotals],
) -> Result<(), sqlx::Error> {
    for (position, ((line, product_id), totals)) in lines
        .iter()
        .zip(product_ids.iter())
        .zip(computed.iter())
        .enumerate()
    {
        let line_id = new_uuid_v7();
        let line_public = PublicId::new(IdKind::Quote, line_id)
            .as_str()
            .replacen("qte_", "qtl_", 1);
        sqlx::query(
            r#"
            INSERT INTO sales_quote_line (
                id, org_id, public_id, quote_id, position, product_id, description,
                quantity, unit_price_minor, discount_minor, tax_rate_bps, tax_minor, line_total_minor
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
            "#,
        )
        .bind(line_id)
        .bind(org_id)
        .bind(&line_public)
        .bind(quote_id)
        .bind(position as i32)
        .bind(product_id)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(line.unit_price_minor)
        .bind(line.discount_minor)
        .bind(line.tax_rate_bps)
        .bind(totals.tax_minor)
        .bind(totals.line_total_minor)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn push_filters(
    qb: &mut QueryBuilder<'_, Postgres>,
    scope: companyos_authz::Scope,
    org_id: Uuid,
    actor: Uuid,
    team_id: Option<Uuid>,
    department_id: Option<Uuid>,
    q: Option<&str>,
) {
    push_owner_predicate(qb, scope, org_id, actor, team_id, department_id);
    if let Some(q) = q.map(str::trim).filter(|s| !s.is_empty()) {
        qb.push(" AND quote_number ILIKE ");
        qb.push_bind(format!("%{q}%"));
    }
}

/// GET /api/v1/sales/quotes
#[utoipa::path(get, path = "/api/v1/sales/quotes", tag = "sales-quotes",
    responses((status = 200, body = QuoteListResponse)))]
pub async fn list_quotes(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<QuoteListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_quote_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_quote_read());
    let (limit, offset) = super::normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM sales_quote WHERE org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND deleted_at IS NULL");
    push_filters(
        &mut count_qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
        q.q.as_deref(),
    );
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {QUOTE_COLUMNS} FROM sales_quote WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    push_filters(
        &mut qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
        q.q.as_deref(),
    );
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<QuoteRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let lines = fetch_lines(&mut tx, org_id, row.id)
            .await
            .map_err(internal(&request_id))?;
        items.push(assemble_dto(row, lines));
    }
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(QuoteListResponse { items, total }))
}

/// POST /api/v1/sales/quotes
#[utoipa::path(post, path = "/api/v1/sales/quotes", tag = "sales-quotes",
    request_body = CreateQuoteRequest,
    responses((status = 201, body = QuoteDto)))]
pub async fn create_quote(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateQuoteRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
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
        perms::sales_quote_create(),
        &request_id,
    )?;

    let customer_id = parse_public_id(IdKind::Customer, &body.customer_id, &request_id)?;
    let deal_id =
        super::parse_optional_public_id(IdKind::Deal, body.deal_id.as_deref(), &request_id)?;
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => parse_public_id(IdKind::User, s, &request_id)?,
        None => auth.ctx.actor.user_id,
    };
    let currency_code = body.currency.clone().unwrap_or_else(|| "USD".into());
    let currency = Currency::new(&currency_code)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let valid_until = body
        .valid_until
        .as_deref()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| validation(&request_id, "valid_until must be YYYY-MM-DD"))?;

    let product_ids = parse_line_product_ids(&body.lines, &request_id)?;
    let computed = compute_lines(&body.lines, currency, &request_id)?;
    let doc = crate::quotes_math::sum_document(&computed)
        .map_err(|e| validation(&request_id, format!("invalid quote totals: {e}")))?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Quote, id);
    let quote_number = body
        .quote_number
        .clone()
        .unwrap_or_else(|| format!("Q-{}", id.simple().to_string()[..8].to_uppercase()));

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "quote.create", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    sqlx::query(
        r#"
        INSERT INTO sales_quote (
            id, org_id, public_id, deal_id, customer_id, quote_number, status, version_number,
            currency, subtotal_minor, discount_minor, tax_minor, total_minor, notes, valid_until, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,'draft',1,$7,$8,$9,$10,$11,$12,$13,$14)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(deal_id)
    .bind(customer_id)
    .bind(&quote_number)
    .bind(currency_code.as_str())
    .bind(doc.subtotal_minor)
    .bind(doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .bind(&body.notes)
    .bind(valid_until)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_lines(&mut tx, org_id, id, &body.lines, &product_ids, &computed)
        .await
        .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "quote",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": public_id.as_str(), "total_minor": doc.total_minor, "currency": currency_code }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let dto = fetch_quote_dto(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "quote missing after insert",
            )
        })?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "quote.create",
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

async fn enforce_quote_scope(
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

/// GET /api/v1/sales/quotes/{id}
#[utoipa::path(get, path = "/api/v1/sales/quotes/{id}", tag = "sales-quotes",
    responses((status = 200, body = QuoteDto), (status = 404)))]
pub async fn get_quote(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<QuoteDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let quote_id = parse_public_id(IdKind::Quote, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_quote_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_quote_row(&mut tx, org_id, quote_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "quote"))?;
    enforce_quote_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_quote_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    let lines = fetch_lines(&mut tx, org_id, row.id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(assemble_dto(row, lines)))
}

/// PATCH /api/v1/sales/quotes/{id}
///
/// If the current quote is `accepted`, this creates a **new** draft version
/// (`version_number + 1`, `previous_quote_id` set to the accepted quote) and
/// leaves the accepted row untouched. Otherwise the draft is updated in place.
#[utoipa::path(patch, path = "/api/v1/sales/quotes/{id}", tag = "sales-quotes",
    request_body = UpdateQuoteRequest,
    responses((status = 200, body = QuoteDto), (status = 404)))]
pub async fn update_quote(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateQuoteRequest>,
) -> Result<(StatusCode, Json<QuoteDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let quote_id = parse_public_id(IdKind::Quote, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_quote_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_quote_row(&mut tx, org_id, quote_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "quote"))?;
    enforce_quote_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_quote_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    let currency_code = body
        .currency
        .clone()
        .unwrap_or_else(|| row.currency.clone());
    let currency = Currency::new(&currency_code)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let notes = body.notes.clone().or(row.notes.clone());
    let valid_until = match body.valid_until.as_deref() {
        Some(s) => Some(
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| validation(&request_id, "valid_until must be YYYY-MM-DD"))?,
        ),
        None => row.valid_until,
    };
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => row.owner_user_id,
    };

    let existing_lines = fetch_lines(&mut tx, org_id, row.id)
        .await
        .map_err(internal(&request_id))?;
    let line_requests: Vec<CreateQuoteLineRequest> = match &body.lines {
        Some(lines) => lines.clone(),
        None => existing_lines
            .iter()
            .cloned()
            .map(|l| CreateQuoteLineRequest {
                product_id: l
                    .product_id
                    .map(|u| PublicId::new(IdKind::Product, u).as_str()),
                description: l.description,
                quantity: l.quantity,
                unit_price_minor: l.unit_price_minor,
                discount_minor: l.discount_minor,
                tax_rate_bps: l.tax_rate_bps,
            })
            .collect(),
    };
    let product_ids = parse_line_product_ids(&line_requests, &request_id)?;
    let computed = compute_lines(&line_requests, currency, &request_id)?;
    let doc = crate::quotes_math::sum_document(&computed)
        .map_err(|e| validation(&request_id, format!("invalid quote totals: {e}")))?;

    if row.status == "accepted" {
        // Immutable after acceptance: fork a new draft version instead of mutating.
        let new_id = new_uuid_v7();
        let new_public = PublicId::new(IdKind::Quote, new_id);
        sqlx::query(
            r#"
            INSERT INTO sales_quote (
                id, org_id, public_id, deal_id, customer_id, quote_number, status, version_number,
                previous_quote_id, currency, subtotal_minor, discount_minor, tax_minor, total_minor,
                notes, valid_until, owner_user_id
            ) VALUES ($1,$2,$3,$4,$5,$6,'draft',$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
            "#,
        )
        .bind(new_id)
        .bind(org_id)
        .bind(new_public.as_str())
        .bind(row.deal_id)
        .bind(row.customer_id)
        .bind(&row.quote_number)
        .bind(row.version_number + 1)
        .bind(row.id)
        .bind(currency_code.as_str())
        .bind(doc.subtotal_minor)
        .bind(doc.discount_minor)
        .bind(doc.tax_minor)
        .bind(doc.total_minor)
        .bind(&notes)
        .bind(valid_until)
        .bind(owner_user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        insert_lines(
            &mut tx,
            org_id,
            new_id,
            &line_requests,
            &product_ids,
            &computed,
        )
        .await
        .map_err(internal(&request_id))?;

        insert_audit(
            &mut *tx,
            org_id,
            auth.ctx.actor.user_id,
            auth.ctx.actor.on_behalf_of,
            auth.ctx.actor.is_ai,
            "sales.quote.new_version",
            "quote",
            &new_public.as_str(),
            serde_json::json!({ "previous_quote_id": row.public_id }),
        )
        .await
        .map_err(internal(&request_id))?;

        let dto = fetch_quote_dto(&mut tx, org_id, new_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::Internal,
                    request_id.clone(),
                    "quote missing after insert",
                )
            })?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((StatusCode::CREATED, Json(dto)));
    }

    sqlx::query(
        r#"
        UPDATE sales_quote
        SET currency = $3, subtotal_minor = $4, discount_minor = $5, tax_minor = $6, total_minor = $7,
            notes = $8, valid_until = $9, owner_user_id = $10, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(row.id)
    .bind(currency_code.as_str())
    .bind(doc.subtotal_minor)
    .bind(doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .bind(&notes)
    .bind(valid_until)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if body.lines.is_some() {
        sqlx::query("DELETE FROM sales_quote_line WHERE org_id = $1 AND quote_id = $2")
            .bind(org_id)
            .bind(row.id)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        insert_lines(
            &mut tx,
            org_id,
            row.id,
            &line_requests,
            &product_ids,
            &computed,
        )
        .await
        .map_err(internal(&request_id))?;
    }

    let dto = fetch_quote_dto(&mut tx, org_id, row.id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "quote missing after update",
            )
        })?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::OK, Json(dto)))
}

/// POST /api/v1/sales/quotes/{id}/send
#[utoipa::path(post, path = "/api/v1/sales/quotes/{id}/send", tag = "sales-quotes",
    responses((status = 200, body = QuoteDto)))]
pub async fn send_quote(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<QuoteDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let quote_id = parse_public_id(IdKind::Quote, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_quote_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_quote_row(&mut tx, org_id, quote_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "quote"))?;
    enforce_quote_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_quote_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == "accepted" || row.status == "rejected" {
        return Err(conflict(
            &request_id,
            format!("quote is {}, cannot send", row.status),
        ));
    }

    // Local-only accept link — no email integration in this phase.
    let accept_link = format!("/api/v1/sales/quotes/{}/accept", row.public_id);
    let notes = match &row.notes {
        Some(existing) => format!("{existing}\n[sent] accept link: {accept_link}"),
        None => format!("[sent] accept link: {accept_link}"),
    };

    sqlx::query(
        "UPDATE sales_quote SET status = 'sent', notes = $3, version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(row.id)
    .bind(&notes)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_quote_dto(&mut tx, org_id, row.id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "quote missing after send",
            )
        })?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/sales/quotes/{id}/accept
#[utoipa::path(post, path = "/api/v1/sales/quotes/{id}/accept", tag = "sales-quotes",
    responses((status = 200, body = QuoteDto)))]
pub async fn accept_quote(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<QuoteDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let quote_id = parse_public_id(IdKind::Quote, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_quote_accept(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_quote_row(&mut tx, org_id, quote_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "quote"))?;
    enforce_quote_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_quote_accept(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == "accepted" {
        // Idempotent: already accepted, no second event.
        let dto = fetch_quote_dto(&mut tx, org_id, row.id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| {
                AppError::new(ErrorCode::Internal, request_id.clone(), "quote missing")
            })?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(dto));
    }
    if row.status == "rejected" {
        return Err(conflict(&request_id, "quote already rejected"));
    }

    sqlx::query(
        "UPDATE sales_quote SET status = 'accepted', accepted_at = now(), version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(row.id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "quote",
        "accepted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": row.public_id, "total_minor": row.total_minor, "currency": row.currency }),
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
        "sales.quote.accept",
        "quote",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_quote_dto(&mut tx, org_id, row.id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "quote missing after accept",
            )
        })?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/sales/quotes/{id}/reject
#[utoipa::path(post, path = "/api/v1/sales/quotes/{id}/reject", tag = "sales-quotes",
    request_body = RejectQuoteRequest,
    responses((status = 200, body = QuoteDto)))]
pub async fn reject_quote(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<RejectQuoteRequest>,
) -> Result<Json<QuoteDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let quote_id = parse_public_id(IdKind::Quote, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_quote_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_quote_row(&mut tx, org_id, quote_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "quote"))?;
    enforce_quote_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_quote_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == "accepted" {
        return Err(conflict(&request_id, "accepted quote cannot be rejected"));
    }
    if row.status == "rejected" {
        let dto = fetch_quote_dto(&mut tx, org_id, row.id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| {
                AppError::new(ErrorCode::Internal, request_id.clone(), "quote missing")
            })?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(dto));
    }

    let notes = match (&row.notes, &body.reason) {
        (Some(existing), Some(reason)) => Some(format!("{existing}\n[rejected] {reason}")),
        (None, Some(reason)) => Some(format!("[rejected] {reason}")),
        (existing, None) => existing.clone(),
    };

    sqlx::query(
        "UPDATE sales_quote SET status = 'rejected', notes = $3, version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(row.id)
    .bind(&notes)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_quote_dto(&mut tx, org_id, row.id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "quote missing after reject",
            )
        })?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// GET /api/v1/sales/quotes/{id}/invoice-action
///
/// Available when the quote is accepted — Finance creates the invoice from a
/// quote snapshot (no CRM table reads on the finance side).
#[utoipa::path(get, path = "/api/v1/sales/quotes/{id}/invoice-action", tag = "sales-quotes",
    responses((status = 200, body = InvoiceActionResponse)))]
pub async fn invoice_action(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<InvoiceActionResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let quote_uuid = parse_public_id(IdKind::Quote, &id, &request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    let row: Option<(String,)> =
        sqlx::query_as("SELECT status FROM sales_quote WHERE org_id = $1 AND id = $2")
            .bind(auth.ctx.org_id.as_uuid())
            .bind(quote_uuid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    let Some((status,)) = row else {
        return Ok(Json(InvoiceActionResponse {
            available: false,
            reason: "quote_not_found".into(),
        }));
    };
    if status == "accepted" {
        Ok(Json(InvoiceActionResponse {
            available: true,
            reason: "ready".into(),
        }))
    } else {
        Ok(Json(InvoiceActionResponse {
            available: false,
            reason: format!("quote_status_{status}"),
        }))
    }
}
