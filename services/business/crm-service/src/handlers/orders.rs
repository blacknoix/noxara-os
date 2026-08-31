//! `/api/v1/sales/orders` — sales orders from quotes/deals and status updates.

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
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{conflict, internal, is_unique_violation, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{
    enforce_any_scope, load_membership_scope_for, required_scope_for_owner_row, MembershipScope,
};
use crate::quotes_math::{compute_quote_totals, LineInput};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreateOrderLineRequest, CreateOrderRequest, OrderDto, OrderFromDealRequest,
    OrderFromQuoteRequest, OrderLineDto, OrderListQuery, OrderListResponse,
    UpdateOrderStatusRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sales/orders", get(list_orders).post(create_order))
        .route(
            "/api/v1/sales/orders/from-quote",
            post(order_from_quote),
        )
        .route("/api/v1/sales/orders/from-deal", post(order_from_deal))
        .route(
            "/api/v1/sales/orders/{id}",
            get(get_order).patch(update_order_status),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct OrderRow {
    id: Uuid,
    public_id: String,
    customer_id: Uuid,
    deal_id: Option<Uuid>,
    quote_id: Option<Uuid>,
    status: String,
    currency: String,
    subtotal_minor: i64,
    discount_minor: i64,
    tax_minor: i64,
    total_minor: i64,
    owner_user_id: Option<Uuid>,
    territory_id: Option<Uuid>,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const ORDER_COLUMNS: &str = "id, public_id, customer_id, deal_id, quote_id, status, currency, subtotal_minor, discount_minor, tax_minor, total_minor, owner_user_id, territory_id, notes, created_at, updated_at, version";

#[derive(Debug, Clone, sqlx::FromRow)]
struct OrderLineRow {
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

const ORDER_LINE_COLUMNS: &str = "public_id, position, product_id, description, quantity, unit_price_minor, discount_minor, tax_rate_bps, tax_minor, line_total_minor";

impl OrderLineRow {
    fn into_dto(self) -> OrderLineDto {
        OrderLineDto {
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

fn assemble_dto(row: OrderRow, lines: Vec<OrderLineRow>) -> OrderDto {
    OrderDto {
        id: row.public_id,
        customer_id: PublicId::new(IdKind::Customer, row.customer_id).as_str(),
        deal_id: row.deal_id.map(|u| PublicId::new(IdKind::Deal, u).as_str()),
        quote_id: row
            .quote_id
            .map(|u| PublicId::new(IdKind::Quote, u).as_str()),
        status: row.status,
        currency: row.currency,
        subtotal_minor: row.subtotal_minor,
        discount_minor: row.discount_minor,
        tax_minor: row.tax_minor,
        total_minor: row.total_minor,
        owner_user_id: row
            .owner_user_id
            .map(|u| PublicId::new(IdKind::User, u).as_str()),
        territory_id: row
            .territory_id
            .map(|u| PublicId::new(IdKind::Territory, u).as_str()),
        notes: row.notes,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        version: row.version,
        lines: lines.into_iter().map(OrderLineRow::into_dto).collect(),
    }
}

async fn fetch_lines(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    order_id: Uuid,
) -> Result<Vec<OrderLineRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {ORDER_LINE_COLUMNS} FROM sales_order_line WHERE org_id = $1 AND order_id = $2 ORDER BY position ASC"
    ))
    .bind(org_id)
    .bind(order_id)
    .fetch_all(&mut **tx)
    .await
}

async fn fetch_order_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    order_id: Uuid,
) -> Result<Option<OrderRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {ORDER_COLUMNS} FROM sales_order WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(order_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn fetch_order_dto(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    order_id: Uuid,
) -> Result<Option<OrderDto>, sqlx::Error> {
    let Some(row) = fetch_order_row(tx, org_id, order_id).await? else {
        return Ok(None);
    };
    let lines = fetch_lines(tx, org_id, row.id).await?;
    Ok(Some(assemble_dto(row, lines)))
}

fn compute_lines(
    lines: &[CreateOrderLineRequest],
    currency: Currency,
    request_id: &str,
) -> Result<(Vec<crate::quotes_math::LineTotals>, crate::quotes_math::DocumentTotals), AppError> {
    let inputs: Vec<LineInput> = lines
        .iter()
        .map(|l| LineInput {
            quantity: l.quantity as i64,
            unit_price_minor: l.unit_price_minor,
            discount_minor: l.discount_minor,
            tax_rate_bps: l.tax_rate_bps as i64,
        })
        .collect();
    compute_quote_totals(&inputs, currency).map_err(|e| {
        validation(request_id, format!("money calculation failed: {e}"))
    })
}

async fn insert_order_lines(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    order_id: Uuid,
    lines: &[CreateOrderLineRequest],
    computed: &[crate::quotes_math::LineTotals],
    request_id: &str,
) -> Result<(), AppError> {
    for (idx, (line, totals)) in lines.iter().zip(computed.iter()).enumerate() {
        let line_id = new_uuid_v7();
        let line_public = PublicId::new(IdKind::SalesOrderLine, line_id);
        let product_id = super::parse_optional_public_id(
            IdKind::Product,
            line.product_id.as_deref(),
            request_id,
        )?;
        sqlx::query(
            r#"
            INSERT INTO sales_order_line (
                id, org_id, public_id, order_id, position, product_id, description,
                quantity, unit_price_minor, discount_minor, tax_rate_bps, tax_minor, line_total_minor
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
            "#,
        )
        .bind(line_id)
        .bind(org_id)
        .bind(line_public.as_str())
        .bind(order_id)
        .bind(idx as i32)
        .bind(product_id)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(line.unit_price_minor)
        .bind(totals.discount_minor)
        .bind(line.tax_rate_bps)
        .bind(totals.tax_minor)
        .bind(totals.line_total_minor)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }
    Ok(())
}

async fn emit_order_created(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    auth: &AuthCtx,
    dto: &OrderDto,
    request_id: &str,
) -> Result<(), AppError> {
    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "order",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "customer_id": dto.customer_id,
            "quote_id": dto.quote_id,
            "deal_id": dto.deal_id,
            "total_minor": dto.total_minor,
            "currency": dto.currency,
            "status": dto.status,
        }),
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}

async fn enforce_order_scope(
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

fn valid_status_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("draft", "confirmed")
            | ("draft", "cancelled")
            | ("confirmed", "fulfilled")
            | ("confirmed", "cancelled")
    )
}

/// GET /api/v1/sales/orders
#[utoipa::path(get, path = "/api/v1/sales/orders", tag = "sales-orders",
    responses((status = 200, body = OrderListResponse)))]
pub async fn list_orders(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<OrderListQuery>,
) -> Result<Json<OrderListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_order_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_order_read());
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
        QueryBuilder::new("SELECT COUNT(*) FROM sales_order WHERE org_id = ");
    count_qb.push_bind(org_id);
    build_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {ORDER_COLUMNS} FROM sales_order WHERE org_id = "
    ));
    qb.push_bind(org_id);
    build_filters(&mut qb);
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<OrderRow> = qb
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

    Ok(Json(OrderListResponse { items, total }))
}

/// POST /api/v1/sales/orders
#[utoipa::path(post, path = "/api/v1/sales/orders", tag = "sales-orders",
    request_body = CreateOrderRequest,
    responses((status = 201, body = OrderDto)))]
pub async fn create_order(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateOrderRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_order_create(),
        &request_id,
    )?;

    let currency_str = body.currency.clone().unwrap_or_else(|| "USD".into());
    let currency = Currency::new(&currency_str)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let (computed, doc) = compute_lines(&body.lines, currency, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "order.create", key)
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
    let quote_id =
        super::parse_optional_public_id(IdKind::Quote, body.quote_id.as_deref(), &request_id)?;
    let territory_id = super::parse_optional_public_id(
        IdKind::Territory,
        body.territory_id.as_deref(),
        &request_id,
    )?;
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => parse_public_id(IdKind::User, s, &request_id)?,
        None => auth.ctx.actor.user_id,
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::SalesOrder, id);

    let insert = sqlx::query(
        r#"
        INSERT INTO sales_order (
            id, org_id, public_id, customer_id, deal_id, quote_id, status, currency,
            subtotal_minor, discount_minor, tax_minor, total_minor, owner_user_id, territory_id, notes
        ) VALUES ($1,$2,$3,$4,$5,$6,'draft',$7,$8,$9,$10,$11,$12,$13,$14)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(customer_id)
    .bind(deal_id)
    .bind(quote_id)
    .bind(&currency_str)
    .bind(doc.subtotal_minor)
    .bind(doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .bind(owner_user_id)
    .bind(territory_id)
    .bind(&body.notes)
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert {
        if is_unique_violation(&e, "sales_order_org_quote_unique_idx") {
            return Err(conflict(
                &request_id,
                "an order already exists for this quote",
            ));
        }
        return Err(internal(&request_id)(e));
    }

    insert_order_lines(&mut tx, org_id, id, &body.lines, &computed, &request_id).await?;

    let dto = fetch_order_dto(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "order missing after insert",
            )
        })?;

    emit_order_created(&mut tx, &auth, &dto, &request_id).await?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "order.create",
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

/// GET /api/v1/sales/orders/{id}
#[utoipa::path(get, path = "/api/v1/sales/orders/{id}", tag = "sales-orders",
    responses((status = 200, body = OrderDto), (status = 404)))]
pub async fn get_order(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<OrderDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let order_id = parse_public_id(IdKind::SalesOrder, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_order_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_order_row(&mut tx, org_id, order_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "order"))?;
    enforce_order_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_order_read(),
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

/// PATCH /api/v1/sales/orders/{id} — status transitions only.
#[utoipa::path(patch, path = "/api/v1/sales/orders/{id}", tag = "sales-orders",
    request_body = UpdateOrderStatusRequest,
    responses((status = 200, body = OrderDto), (status = 404), (status = 409)))]
pub async fn update_order_status(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateOrderStatusRequest>,
) -> Result<Json<OrderDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let order_id = parse_public_id(IdKind::SalesOrder, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_order_update(),
        &request_id,
    )?;

    let new_status = body.status.trim();
    if !matches!(
        new_status,
        "draft" | "confirmed" | "fulfilled" | "cancelled"
    ) {
        return Err(validation(
            &request_id,
            "status must be draft|confirmed|fulfilled|cancelled",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_order_row(&mut tx, org_id, order_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "order"))?;
    enforce_order_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_order_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status == new_status {
        let lines = fetch_lines(&mut tx, org_id, row.id)
            .await
            .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(assemble_dto(row, lines)));
    }

    if !valid_status_transition(&row.status, new_status) {
        return Err(conflict(
            &request_id,
            format!(
                "cannot transition order status from {} to {new_status}",
                row.status
            ),
        ));
    }

    let updated: OrderRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_order
        SET status = $3, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {ORDER_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(order_id)
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
        "sales.order.update",
        "order",
        &updated.public_id,
        serde_json::json!({ "status": new_status }),
    )
    .await
    .map_err(internal(&request_id))?;

    let lines = fetch_lines(&mut tx, org_id, updated.id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(assemble_dto(updated, lines)))
}

/// POST /api/v1/sales/orders/from-quote — create order from an accepted quote.
#[utoipa::path(post, path = "/api/v1/sales/orders/from-quote", tag = "sales-orders",
    request_body = OrderFromQuoteRequest,
    responses((status = 201, body = OrderDto), (status = 409)))]
pub async fn order_from_quote(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<OrderFromQuoteRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_order_create(),
        &request_id,
    )?;

    let quote_id = parse_public_id(IdKind::Quote, &body.quote_id, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "order.from_quote", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    #[derive(sqlx::FromRow)]
    struct QuoteHead {
        id: Uuid,
        customer_id: Uuid,
        deal_id: Option<Uuid>,
        status: String,
        currency: String,
        subtotal_minor: i64,
        discount_minor: i64,
        tax_minor: i64,
        total_minor: i64,
        notes: Option<String>,
        owner_user_id: Option<Uuid>,
    }

    let quote: QuoteHead = sqlx::query_as(
        r#"
        SELECT id, customer_id, deal_id, status, currency, subtotal_minor, discount_minor,
               tax_minor, total_minor, notes, owner_user_id
        FROM sales_quote
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(quote_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .ok_or_else(|| not_found(&request_id, "quote"))?;

    if quote.status != "accepted" {
        return Err(validation(
            &request_id,
            "order can only be created from an accepted quote",
        ));
    }

    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM sales_order WHERE org_id = $1 AND quote_id = $2 LIMIT 1",
    )
    .bind(org_id)
    .bind(quote_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    if existing.is_some() {
        return Err(conflict(
            &request_id,
            "an order already exists for this quote",
        ));
    }

    #[derive(sqlx::FromRow)]
    struct QuoteLine {
        product_id: Option<Uuid>,
        description: String,
        quantity: i32,
        unit_price_minor: i64,
        discount_minor: i64,
        tax_rate_bps: i32,
        #[allow(dead_code)]
        tax_minor: i64,
        #[allow(dead_code)]
        line_total_minor: i64,
        position: i32,
    }

    let qlines: Vec<QuoteLine> = sqlx::query_as(
        r#"
        SELECT product_id, description, quantity, unit_price_minor, discount_minor,
               tax_rate_bps, tax_minor, line_total_minor, position
        FROM sales_quote_line
        WHERE org_id = $1 AND quote_id = $2
        ORDER BY position ASC
        "#,
    )
    .bind(org_id)
    .bind(quote.id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    // Recompute with quotes_math to guarantee exact money totals.
    let currency = Currency::new(&quote.currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let inputs: Vec<LineInput> = qlines
        .iter()
        .map(|l| LineInput {
            quantity: l.quantity as i64,
            unit_price_minor: l.unit_price_minor,
            discount_minor: l.discount_minor,
            tax_rate_bps: l.tax_rate_bps as i64,
        })
        .collect();
    let (computed, doc) = compute_quote_totals(&inputs, currency).map_err(|e| {
        validation(&request_id, format!("money calculation failed: {e}"))
    })?;

    // Prefer recomputed totals; they must match stored quote totals for accepted quotes.
    if doc.subtotal_minor != quote.subtotal_minor
        || doc.discount_minor != quote.discount_minor
        || doc.tax_minor != quote.tax_minor
        || doc.total_minor != quote.total_minor
    {
        // Still use recomputed values — they are the source of truth for order lines.
        tracing::warn!(
            quote_id = %body.quote_id,
            "quote stored totals diverge from recomputed; using recomputed for order"
        );
    }

    let order_id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::SalesOrder, order_id);
    let owner = quote.owner_user_id.unwrap_or(auth.ctx.actor.user_id);

    sqlx::query(
        r#"
        INSERT INTO sales_order (
            id, org_id, public_id, customer_id, deal_id, quote_id, status, currency,
            subtotal_minor, discount_minor, tax_minor, total_minor, owner_user_id, notes
        ) VALUES ($1,$2,$3,$4,$5,$6,'draft',$7,$8,$9,$10,$11,$12,$13)
        "#,
    )
    .bind(order_id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(quote.customer_id)
    .bind(quote.deal_id)
    .bind(quote.id)
    .bind(&quote.currency)
    .bind(doc.subtotal_minor)
    .bind(doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .bind(owner)
    .bind(&quote.notes)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if is_unique_violation(&e, "sales_order_org_quote_unique_idx") {
            conflict(&request_id, "an order already exists for this quote")
        } else {
            internal(&request_id)(e)
        }
    })?;

    for (idx, (line, totals)) in qlines.iter().zip(computed.iter()).enumerate() {
        let line_id = new_uuid_v7();
        let line_public = PublicId::new(IdKind::SalesOrderLine, line_id);
        sqlx::query(
            r#"
            INSERT INTO sales_order_line (
                id, org_id, public_id, order_id, position, product_id, description,
                quantity, unit_price_minor, discount_minor, tax_rate_bps, tax_minor, line_total_minor
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
            "#,
        )
        .bind(line_id)
        .bind(org_id)
        .bind(line_public.as_str())
        .bind(order_id)
        .bind(line.position.max(idx as i32))
        .bind(line.product_id)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(line.unit_price_minor)
        .bind(totals.discount_minor)
        .bind(line.tax_rate_bps)
        .bind(totals.tax_minor)
        .bind(totals.line_total_minor)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let dto = fetch_order_dto(&mut tx, org_id, order_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "order missing after insert",
            )
        })?;

    emit_order_created(&mut tx, &auth, &dto, &request_id).await?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "order.from_quote",
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

/// POST /api/v1/sales/orders/from-deal — single-line order from a won deal.
#[utoipa::path(post, path = "/api/v1/sales/orders/from-deal", tag = "sales-orders",
    request_body = OrderFromDealRequest,
    responses((status = 201, body = OrderDto)))]
pub async fn order_from_deal(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<OrderFromDealRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_order_create(),
        &request_id,
    )?;

    let deal_id = parse_public_id(IdKind::Deal, &body.deal_id, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "order.from_deal", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let deal = crate::handlers::deals::fetch_deal_row(&mut tx, org_id, deal_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "deal"))?;

    if deal.status != "won" {
        return Err(validation(
            &request_id,
            "order can only be created from a won deal",
        ));
    }

    let customer_id = deal.customer_id.ok_or_else(|| {
        validation(
            &request_id,
            "won deal must have a customer_id to create an order",
        )
    })?;

    let line = CreateOrderLineRequest {
        product_id: None,
        description: deal.name.clone(),
        quantity: 1,
        unit_price_minor: deal.amount_minor,
        discount_minor: 0,
        tax_rate_bps: 0,
    };
    let currency = Currency::new(&deal.currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let (computed, doc) = compute_lines(&[line.clone()], currency, &request_id)?;

    let order_id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::SalesOrder, order_id);
    let owner = deal.owner_user_id.unwrap_or(auth.ctx.actor.user_id);

    sqlx::query(
        r#"
        INSERT INTO sales_order (
            id, org_id, public_id, customer_id, deal_id, quote_id, status, currency,
            subtotal_minor, discount_minor, tax_minor, total_minor, owner_user_id, notes
        ) VALUES ($1,$2,$3,$4,$5,NULL,'draft',$6,$7,$8,$9,$10,$11,$12)
        "#,
    )
    .bind(order_id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(customer_id)
    .bind(deal_id)
    .bind(&deal.currency)
    .bind(doc.subtotal_minor)
    .bind(doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .bind(owner)
    .bind(Option::<String>::None)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_order_lines(
        &mut tx,
        org_id,
        order_id,
        &[line],
        &computed,
        &request_id,
    )
    .await?;

    let dto = fetch_order_dto(&mut tx, org_id, order_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "order missing after insert",
            )
        })?;

    emit_order_created(&mut tx, &auth, &dto, &request_id).await?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "order.from_deal",
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
