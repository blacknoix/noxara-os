//! `/api/v1/inventory/purchase-orders` — create (optionally from an approved
//! purchase request) → issue.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::{conflict, internal, normalize_paging, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreatePurchaseOrderRequest, ListQuery, PurchaseOrderDto, PurchaseOrderLineDto,
    PurchaseOrderListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/purchase-orders",
            get(list_purchase_orders).post(create_purchase_order),
        )
        .route(
            "/api/v1/inventory/purchase-orders/{id}",
            get(get_purchase_order),
        )
        .route(
            "/api/v1/inventory/purchase-orders/{id}/issue",
            post(issue_purchase_order),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PoRow {
    id: Uuid,
    public_id: String,
    supplier_public_id: String,
    purchase_request_public_id: Option<String>,
    status: String,
    currency: String,
    total_amount_minor: i64,
    issued_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const PO_COLS: &str = r#"
    po.id, po.public_id, s.public_id AS supplier_public_id, pr.public_id AS purchase_request_public_id,
    po.status, po.currency, po.total_amount_minor, po.issued_at, po.created_at, po.updated_at, po.version
"#;

#[derive(sqlx::FromRow)]
struct PoLineRow {
    public_id: String,
    item_public_id: String,
    warehouse_public_id: String,
    qty_ordered: i64,
    qty_received: i64,
    unit_cost_minor: i64,
    line_amount_minor: i64,
}

async fn fetch_po(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    po_id: Uuid,
    request_id: &str,
) -> Result<PoRow, AppError> {
    sqlx::query_as(&format!(
        r#"
        SELECT {PO_COLS}
        FROM inventory_purchase_order po
        JOIN inventory_supplier s ON s.id = po.supplier_id AND s.org_id = po.org_id
        LEFT JOIN inventory_purchase_request pr ON pr.id = po.purchase_request_id AND pr.org_id = po.org_id
        WHERE po.org_id = $1 AND po.id = $2
        "#
    ))
    .bind(org_id)
    .bind(po_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "purchase order"))
}

async fn fetch_po_lines(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    po_id: Uuid,
    request_id: &str,
) -> Result<Vec<PurchaseOrderLineDto>, AppError> {
    let rows: Vec<PoLineRow> = sqlx::query_as(
        r#"
        SELECT l.public_id, i.public_id AS item_public_id, w.public_id AS warehouse_public_id,
               l.qty_ordered, l.qty_received, l.unit_cost_minor, l.line_amount_minor
        FROM inventory_purchase_order_line l
        JOIN inventory_item i ON i.id = l.item_id AND i.org_id = l.org_id
        JOIN inventory_warehouse w ON w.id = l.warehouse_id AND w.org_id = l.org_id
        WHERE l.org_id = $1 AND l.order_id = $2
        ORDER BY l.created_at
        "#,
    )
    .bind(org_id)
    .bind(po_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(rows
        .into_iter()
        .map(|r| PurchaseOrderLineDto {
            id: r.public_id,
            item_id: r.item_public_id,
            warehouse_id: r.warehouse_public_id,
            qty_ordered: r.qty_ordered,
            qty_received: r.qty_received,
            unit_cost_minor: r.unit_cost_minor,
            line_amount_minor: r.line_amount_minor,
        })
        .collect())
}

async fn to_dto(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    row: PoRow,
    request_id: &str,
) -> Result<PurchaseOrderDto, AppError> {
    let lines = fetch_po_lines(tx, org_id, row.id, request_id).await?;
    Ok(PurchaseOrderDto {
        id: row.public_id,
        supplier_id: row.supplier_public_id,
        purchase_request_id: row.purchase_request_public_id,
        status: row.status,
        currency: row.currency,
        total_amount_minor: row.total_amount_minor,
        issued_at: row.issued_at.map(|t| t.to_rfc3339()),
        lines,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        version: row.version,
    })
}

/// GET /api/v1/inventory/purchase-orders
#[utoipa::path(get, path = "/api/v1/inventory/purchase-orders", tag = "inventory-purchase-orders",
    params(ListQuery), responses((status = 200, body = PurchaseOrderListResponse)))]
pub async fn list_purchase_orders(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<PurchaseOrderListResponse>, AppError> {
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
    let perm = perms::inventory_purchase_order_read();
    enforce_any_scope(&membership.principal, perm.clone(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perm);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM inventory_purchase_order po WHERE po.org_id = ");
    count_qb.push_bind(org_id);
    push_owner_predicate(
        &mut count_qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        r#"
        SELECT {PO_COLS}
        FROM inventory_purchase_order po
        JOIN inventory_supplier s ON s.id = po.supplier_id AND s.org_id = po.org_id
        LEFT JOIN inventory_purchase_request pr ON pr.id = po.purchase_request_id AND pr.org_id = po.org_id
        WHERE po.org_id = "#
    ));
    qb.push_bind(org_id);
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    qb.push(" ORDER BY po.created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<PoRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(to_dto(&mut tx, org_id, row, &request_id).await?);
    }
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(PurchaseOrderListResponse { items, total }))
}

/// POST /api/v1/inventory/purchase-orders
#[utoipa::path(post, path = "/api/v1/inventory/purchase-orders", tag = "inventory-purchase-orders",
    request_body = CreatePurchaseOrderRequest, responses((status = 201, body = PurchaseOrderDto)))]
pub async fn create_purchase_order(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreatePurchaseOrderRequest>,
) -> Result<(axum::http::StatusCode, Json<PurchaseOrderDto>), AppError> {
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
        perms::inventory_purchase_order_write(),
        &request_id,
    )?;

    if body.lines.is_empty() {
        return Err(validation(&request_id, "at least one line is required"));
    }
    let _currency: companyos_money::Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;
    let supplier_id = parse_public_id(IdKind::Supplier, &body.supplier_id, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let supplier_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM inventory_supplier WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .bind(supplier_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    if supplier_exists.is_none() {
        return Err(not_found(&request_id, "supplier"));
    }

    let mut purchase_request_id: Option<Uuid> = None;
    if let Some(ref pr_raw) = body.purchase_request_id {
        let pr_id = parse_public_id(IdKind::PurchaseRequest, pr_raw, &request_id)?;
        let pr_status: Option<(String,)> =
            sqlx::query_as("SELECT status FROM inventory_purchase_request WHERE org_id = $1 AND id = $2")
                .bind(org_id)
                .bind(pr_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal(&request_id))?;
        match pr_status {
            None => return Err(not_found(&request_id, "purchase request")),
            Some((status,)) if status != "approved" => {
                return Err(conflict(
                    &request_id,
                    format!("purchase request status {status} is not approved"),
                ));
            }
            _ => {}
        }
        purchase_request_id = Some(pr_id);
    }

    let mut total_amount_minor: i64 = 0;
    let mut resolved_lines = Vec::with_capacity(body.lines.len());
    for line in &body.lines {
        if line.qty_ordered <= 0 {
            return Err(validation(&request_id, "qty_ordered must be > 0"));
        }
        if line.unit_cost_minor < 0 {
            return Err(validation(&request_id, "unit_cost_minor must be >= 0"));
        }
        let item_id = parse_public_id(IdKind::InventoryItem, &line.item_id, &request_id)?;
        let warehouse_id = parse_public_id(IdKind::Warehouse, &line.warehouse_id, &request_id)?;
        let item_exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM inventory_item WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .bind(item_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if item_exists.is_none() {
            return Err(not_found(&request_id, "item"));
        }
        let wh_exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM inventory_warehouse WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .bind(warehouse_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if wh_exists.is_none() {
            return Err(not_found(&request_id, "warehouse"));
        }
        let line_amount = line.qty_ordered.saturating_mul(line.unit_cost_minor);
        total_amount_minor = total_amount_minor.saturating_add(line_amount);
        resolved_lines.push((item_id, warehouse_id, line.qty_ordered, line.unit_cost_minor, line_amount));
    }

    let po_id = new_uuid_v7();
    let po_public_id = PublicId::new(IdKind::PurchaseOrder, po_id);

    sqlx::query(
        r#"
        INSERT INTO inventory_purchase_order (
            id, org_id, public_id, supplier_id, purchase_request_id, status,
            currency, total_amount_minor, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,'draft',$6,$7,$8)
        "#,
    )
    .bind(po_id)
    .bind(org_id)
    .bind(po_public_id.as_str())
    .bind(supplier_id)
    .bind(purchase_request_id)
    .bind(&body.currency)
    .bind(total_amount_minor)
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for (item_id, warehouse_id, qty_ordered, unit_cost_minor, line_amount) in resolved_lines {
        let line_id = new_uuid_v7();
        let line_public_id = PublicId::new(IdKind::PurchaseOrderLine, line_id);
        sqlx::query(
            r#"
            INSERT INTO inventory_purchase_order_line (
                id, org_id, public_id, order_id, item_id, warehouse_id,
                qty_ordered, unit_cost_minor, line_amount_minor
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(line_id)
        .bind(org_id)
        .bind(line_public_id.as_str())
        .bind(po_id)
        .bind(item_id)
        .bind(warehouse_id)
        .bind(qty_ordered)
        .bind(unit_cost_minor)
        .bind(line_amount)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    if let Some(pr_id) = purchase_request_id {
        sqlx::query(
            "UPDATE inventory_purchase_request SET status = 'converted', version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(pr_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let row = fetch_po(&mut tx, org_id, po_id, &request_id).await?;
    let dto = to_dto(&mut tx, org_id, row, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "purchase_order",
        "drafted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "total_amount_minor": dto.total_amount_minor }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.purchase_order.create",
        "purchase_order",
        &dto.id,
        serde_json::json!({ "total_amount_minor": dto.total_amount_minor }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((axum::http::StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/inventory/purchase-orders/{id}
#[utoipa::path(get, path = "/api/v1/inventory/purchase-orders/{id}", tag = "inventory-purchase-orders",
    params(("id" = String, Path)), responses((status = 200, body = PurchaseOrderDto), (status = 404)))]
pub async fn get_purchase_order(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<PurchaseOrderDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let po_id = parse_public_id(IdKind::PurchaseOrder, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_purchase_order_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_po(&mut tx, org_id, po_id, &request_id).await?;
    let dto = to_dto(&mut tx, org_id, row, &request_id).await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/inventory/purchase-orders/{id}/issue
#[utoipa::path(post, path = "/api/v1/inventory/purchase-orders/{id}/issue", tag = "inventory-purchase-orders",
    params(("id" = String, Path)), responses((status = 200, body = PurchaseOrderDto)))]
pub async fn issue_purchase_order(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let po_id = parse_public_id(IdKind::PurchaseOrder, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_purchase_order_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, cached)) = idempotency::get(&mut *tx, org_id, "purchase_order.issue", &key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            return Ok((
                axum::http::StatusCode::from_u16(status as u16).unwrap_or(axum::http::StatusCode::OK),
                Json(cached),
            )
                .into_response());
        }
    }

    let row = fetch_po(&mut tx, org_id, po_id, &request_id).await?;
    if row.status != "draft" {
        if row.status == "issued" {
            let dto = to_dto(&mut tx, org_id, row, &request_id).await?;
            tx.commit().await.map_err(internal(&request_id))?;
            return Ok((axum::http::StatusCode::OK, Json(dto)).into_response());
        }
        return Err(conflict(
            &request_id,
            format!("purchase order status {} cannot be issued", row.status),
        ));
    }

    sqlx::query(
        "UPDATE inventory_purchase_order SET status = 'issued', issued_at = now(), version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(po_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_po(&mut tx, org_id, po_id, &request_id).await?;
    let dto = to_dto(&mut tx, org_id, row, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "purchase_order",
        "issued",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "total_amount_minor": dto.total_amount_minor }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.purchase_order.issue",
        "purchase_order",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let body_json = serde_json::to_value(&dto)
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;
    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(&mut *tx, org_id, "purchase_order.issue", &key, 200, body_json.clone())
            .await
            .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((axum::http::StatusCode::OK, Json(body_json)).into_response())
}
