//! `/api/v1/inventory/goods-receipts` — draft (partial receipt OK) → post.
//!
//! Posting is where the real inventory logic lives: each line becomes an
//! append-only `receipt` stock movement (via [`crate::stock::post_movement`]),
//! the matching PO line's `qty_received` is accumulated, the PO status is
//! recomputed (`issued` → `partially_received` → `received`), and a single
//! Dr Inventory / Cr AP journal is posted to finance-service for the total
//! received value — all inside one transaction. If finance rejects the
//! journal (e.g. a closed fiscal period), the whole post rolls back: no
//! partial stock movements, no partial PO update. The draft→posted status
//! transition is itself the primary idempotency guard (a second `/post` on
//! an already-posted GRN is a no-op); the `Idempotency-Key` header plus a
//! per-line idempotency key on the underlying stock movement are defense in
//! depth for retries that race the transition.

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
use crate::finance_client;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::stock::{self, PostMovementInput};
use crate::types::{
    CreateGoodsReceiptRequest, GoodsReceiptDto, GoodsReceiptLineDto, GoodsReceiptListResponse,
    ListQuery,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/goods-receipts",
            get(list_goods_receipts).post(create_goods_receipt),
        )
        .route(
            "/api/v1/inventory/goods-receipts/{id}",
            get(get_goods_receipt),
        )
        .route(
            "/api/v1/inventory/goods-receipts/{id}/post",
            post(post_goods_receipt),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GrnRow {
    id: Uuid,
    public_id: String,
    purchase_order_id: Uuid,
    po_public_id: String,
    status: String,
    received_at: Option<DateTime<Utc>>,
    journal_public_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const GRN_COLS: &str = r#"
    g.id, g.public_id, g.purchase_order_id, po.public_id AS po_public_id,
    g.status, g.received_at, g.journal_public_id, g.created_at, g.updated_at, g.version
"#;

#[derive(sqlx::FromRow)]
struct GrnLineRow {
    public_id: String,
    po_line_public_id: String,
    item_public_id: String,
    warehouse_public_id: String,
    qty_received: i64,
    unit_cost_minor: i64,
}

async fn fetch_grn(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    grn_id: Uuid,
    request_id: &str,
) -> Result<GrnRow, AppError> {
    sqlx::query_as(&format!(
        r#"
        SELECT {GRN_COLS}
        FROM inventory_goods_receipt g
        JOIN inventory_purchase_order po ON po.id = g.purchase_order_id AND po.org_id = g.org_id
        WHERE g.org_id = $1 AND g.id = $2
        "#
    ))
    .bind(org_id)
    .bind(grn_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "goods receipt"))
}

async fn fetch_grn_lines(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    grn_id: Uuid,
    request_id: &str,
) -> Result<Vec<GrnLineRow>, AppError> {
    sqlx::query_as(
        r#"
        SELECT l.public_id, pl.public_id AS po_line_public_id,
               i.public_id AS item_public_id,
               w.public_id AS warehouse_public_id,
               l.qty_received, l.unit_cost_minor
        FROM inventory_goods_receipt_line l
        JOIN inventory_purchase_order_line pl ON pl.id = l.po_line_id AND pl.org_id = l.org_id
        JOIN inventory_item i ON i.id = l.item_id AND i.org_id = l.org_id
        JOIN inventory_warehouse w ON w.id = l.warehouse_id AND w.org_id = l.org_id
        WHERE l.org_id = $1 AND l.receipt_id = $2
        ORDER BY l.created_at
        "#,
    )
    .bind(org_id)
    .bind(grn_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))
}

async fn to_dto(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    row: GrnRow,
    request_id: &str,
) -> Result<GoodsReceiptDto, AppError> {
    let lines = fetch_grn_lines(tx, org_id, row.id, request_id).await?;
    Ok(GoodsReceiptDto {
        id: row.public_id,
        purchase_order_id: row.po_public_id,
        status: row.status,
        received_at: row.received_at.map(|t| t.to_rfc3339()),
        journal_public_id: row.journal_public_id,
        lines: lines
            .into_iter()
            .map(|l| GoodsReceiptLineDto {
                id: l.public_id,
                po_line_id: l.po_line_public_id,
                item_id: l.item_public_id,
                warehouse_id: l.warehouse_public_id,
                qty_received: l.qty_received,
                unit_cost_minor: l.unit_cost_minor,
            })
            .collect(),
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        version: row.version,
    })
}

/// GET /api/v1/inventory/goods-receipts
#[utoipa::path(get, path = "/api/v1/inventory/goods-receipts", tag = "inventory-goods-receipts",
    params(ListQuery), responses((status = 200, body = GoodsReceiptListResponse)))]
pub async fn list_goods_receipts(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<GoodsReceiptListResponse>, AppError> {
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
        perms::inventory_goods_receipt_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM inventory_goods_receipt g WHERE g.org_id = ");
    count_qb.push_bind(org_id);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        r#"
        SELECT {GRN_COLS}
        FROM inventory_goods_receipt g
        JOIN inventory_purchase_order po ON po.id = g.purchase_order_id AND po.org_id = g.org_id
        WHERE g.org_id = "#
    ));
    qb.push_bind(org_id);
    qb.push(" ORDER BY g.created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<GrnRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(to_dto(&mut tx, org_id, row, &request_id).await?);
    }
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(GoodsReceiptListResponse { items, total }))
}

/// GET /api/v1/inventory/goods-receipts/{id}
#[utoipa::path(get, path = "/api/v1/inventory/goods-receipts/{id}", tag = "inventory-goods-receipts",
    params(("id" = String, Path)), responses((status = 200, body = GoodsReceiptDto), (status = 404)))]
pub async fn get_goods_receipt(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<GoodsReceiptDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let grn_id = parse_public_id(IdKind::GoodsReceipt, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_goods_receipt_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_grn(&mut tx, org_id, grn_id, &request_id).await?;
    let dto = to_dto(&mut tx, org_id, row, &request_id).await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/inventory/goods-receipts
#[utoipa::path(post, path = "/api/v1/inventory/goods-receipts", tag = "inventory-goods-receipts",
    request_body = CreateGoodsReceiptRequest, responses((status = 201, body = GoodsReceiptDto)))]
pub async fn create_goods_receipt(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateGoodsReceiptRequest>,
) -> Result<(axum::http::StatusCode, Json<GoodsReceiptDto>), AppError> {
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
        perms::inventory_goods_receipt_write(),
        &request_id,
    )?;

    if body.lines.is_empty() {
        return Err(validation(&request_id, "at least one line is required"));
    }
    let po_id = parse_public_id(IdKind::PurchaseOrder, &body.purchase_order_id, &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let po_row: Option<(String,)> =
        sqlx::query_as("SELECT status FROM inventory_purchase_order WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(po_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    let Some((po_status,)) = po_row else {
        return Err(not_found(&request_id, "purchase order"));
    };
    if !matches!(po_status.as_str(), "issued" | "partially_received") {
        return Err(conflict(
            &request_id,
            format!("purchase order status {po_status} cannot receive goods"),
        ));
    }

    #[derive(sqlx::FromRow)]
    struct PoLineForReceipt {
        id: Uuid,
        item_id: Uuid,
        warehouse_id: Uuid,
        qty_ordered: i64,
        qty_received: i64,
        unit_cost_minor: i64,
    }

    let mut resolved = Vec::with_capacity(body.lines.len());
    for line in &body.lines {
        if line.qty_received <= 0 {
            return Err(validation(&request_id, "qty_received must be > 0"));
        }
        let po_line_id = parse_public_id(IdKind::PurchaseOrderLine, &line.po_line_id, &request_id)?;
        let po_line: Option<PoLineForReceipt> = sqlx::query_as(
            r#"
            SELECT id, item_id, warehouse_id, qty_ordered, qty_received, unit_cost_minor
            FROM inventory_purchase_order_line
            WHERE org_id = $1 AND id = $2 AND order_id = $3
            "#,
        )
        .bind(org_id)
        .bind(po_line_id)
        .bind(po_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        let Some(po_line) = po_line else {
            return Err(not_found(&request_id, "purchase order line"));
        };
        let remaining = po_line.qty_ordered - po_line.qty_received;
        if line.qty_received > remaining {
            return Err(validation(
                &request_id,
                format!(
                    "qty_received {} exceeds remaining qty {} for line",
                    line.qty_received, remaining
                ),
            ));
        }
        let unit_cost_minor = line.unit_cost_minor.unwrap_or(po_line.unit_cost_minor);
        if unit_cost_minor < 0 {
            return Err(validation(&request_id, "unit_cost_minor must be >= 0"));
        }
        resolved.push((
            po_line.id,
            po_line.item_id,
            po_line.warehouse_id,
            line.qty_received,
            unit_cost_minor,
        ));
    }

    let grn_id = new_uuid_v7();
    let grn_public_id = PublicId::new(IdKind::GoodsReceipt, grn_id);

    sqlx::query(
        r#"
        INSERT INTO inventory_goods_receipt (id, org_id, public_id, purchase_order_id, status, owner_user_id)
        VALUES ($1,$2,$3,$4,'draft',$5)
        "#,
    )
    .bind(grn_id)
    .bind(org_id)
    .bind(grn_public_id.as_str())
    .bind(po_id)
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for (po_line_id, item_id, warehouse_id, qty_received, unit_cost_minor) in resolved {
        let line_id = new_uuid_v7();
        let line_public_id = PublicId::new(IdKind::GoodsReceiptLine, line_id);
        sqlx::query(
            r#"
            INSERT INTO inventory_goods_receipt_line (
                id, org_id, public_id, receipt_id, po_line_id, item_id, warehouse_id,
                qty_received, unit_cost_minor
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(line_id)
        .bind(org_id)
        .bind(line_public_id.as_str())
        .bind(grn_id)
        .bind(po_line_id)
        .bind(item_id)
        .bind(warehouse_id)
        .bind(qty_received)
        .bind(unit_cost_minor)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let row = fetch_grn(&mut tx, org_id, grn_id, &request_id).await?;
    let dto = to_dto(&mut tx, org_id, row, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "goods_receipt",
        "drafted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "purchase_order_id": dto.purchase_order_id }),
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
        "inventory.goods_receipt.create",
        "goods_receipt",
        &dto.id,
        serde_json::json!({ "purchase_order_id": dto.purchase_order_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((axum::http::StatusCode::CREATED, Json(dto)))
}

/// POST /api/v1/inventory/goods-receipts/{id}/post
///
/// Posts each GRN line as a `receipt` stock movement, accumulates the
/// matching PO line's `qty_received`, recomputes the PO status, and posts a
/// single Dr Inventory / Cr AP journal to finance-service for the total
/// received value. All in one transaction: a rejected journal (e.g. a
/// closed fiscal period) rolls back the stock movements and PO update too.
#[utoipa::path(post, path = "/api/v1/inventory/goods-receipts/{id}/post", tag = "inventory-goods-receipts",
    params(("id" = String, Path)), responses((status = 200, body = GoodsReceiptDto), (status = 409)))]
pub async fn post_goods_receipt(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let grn_id = parse_public_id(IdKind::GoodsReceipt, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_goods_receipt_write(),
        &request_id,
    )?;

    let idem_key = idempotency::header_key(&headers);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    if let Some(ref key) = idem_key {
        if let Some((status, cached)) =
            idempotency::get(&mut *tx, org_id, "goods_receipt.post", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            return Ok((
                axum::http::StatusCode::from_u16(status as u16)
                    .unwrap_or(axum::http::StatusCode::OK),
                Json(cached),
            )
                .into_response());
        }
    }

    let row = fetch_grn(&mut tx, org_id, grn_id, &request_id).await?;
    // The draft -> posted transition is the primary idempotency guard: a
    // second /post on an already-posted GRN is a no-op, not an error.
    if row.status == "posted" {
        let dto = to_dto(&mut tx, org_id, row, &request_id).await?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((axum::http::StatusCode::OK, Json(dto)).into_response());
    }
    if row.status != "draft" {
        return Err(conflict(
            &request_id,
            format!("goods receipt status {} cannot be posted", row.status),
        ));
    }

    let po_id = row.purchase_order_id;
    let po_currency: (String,) = sqlx::query_as(
        "SELECT currency FROM inventory_purchase_order WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(po_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let lines = fetch_grn_lines(&mut tx, org_id, grn_id, &request_id).await?;
    if lines.is_empty() {
        return Err(conflict(&request_id, "goods receipt has no lines"));
    }

    // Re-resolve po_line ids as UUIDs (fetch_grn_lines only carries public
    // ids for the response shape) so we can accumulate qty_received per line.
    #[derive(sqlx::FromRow)]
    struct GrnLineForPost {
        po_line_id: Uuid,
        item_id: Uuid,
        warehouse_id: Uuid,
        qty_received: i64,
        unit_cost_minor: i64,
        public_id: String,
    }
    let post_lines: Vec<GrnLineForPost> = sqlx::query_as(
        r#"
        SELECT po_line_id, item_id, warehouse_id, qty_received, unit_cost_minor, public_id
        FROM inventory_goods_receipt_line
        WHERE org_id = $1 AND receipt_id = $2
        ORDER BY created_at
        "#,
    )
    .bind(org_id)
    .bind(grn_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut total_value_minor: i64 = 0;
    for line in &post_lines {
        let movement_idem_key = format!("grn:{}:{}", row.public_id, line.public_id);
        stock::post_movement(
            &mut tx,
            auth.ctx.org_id,
            &auth.ctx.actor,
            PostMovementInput {
                warehouse_id: line.warehouse_id,
                item_id: line.item_id,
                qty_delta: line.qty_received,
                unit_cost_minor: line.unit_cost_minor,
                movement_type: "receipt".to_string(),
                source_type: Some("goods_receipt".to_string()),
                source_id: Some(grn_id),
                idempotency_key: Some(movement_idem_key),
                memo: Some(format!("GRN {} line {}", row.public_id, line.public_id)),
                created_by: auth.ctx.actor.user_id,
            },
            &request_id,
        )
        .await?;

        total_value_minor = total_value_minor
            .saturating_add(line.qty_received.saturating_mul(line.unit_cost_minor));

        sqlx::query(
            "UPDATE inventory_purchase_order_line SET qty_received = qty_received + $3 WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(line.po_line_id)
        .bind(line.qty_received)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let totals: (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(qty_ordered),0)::bigint, COALESCE(SUM(qty_received),0)::bigint
         FROM inventory_purchase_order_line WHERE org_id = $1 AND order_id = $2",
    )
    .bind(org_id)
    .bind(po_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let new_po_status = if totals.1 >= totals.0 {
        "received"
    } else {
        "partially_received"
    };
    sqlx::query(
        "UPDATE inventory_purchase_order SET status = $3, version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(po_id)
    .bind(new_po_status)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    // Dr Inventory / Cr AP for the total received value. Idempotent on
    // (source_type, source_id) at finance's side too, so a retried /post
    // (same GRN, different Idempotency-Key) still yields exactly one journal.
    let journal_public_id = finance_client::post_receipt_journal(
        &auth,
        grn_id,
        &po_currency.0,
        total_value_minor,
        format!("Goods receipt {}", row.public_id),
        idem_key.as_deref(),
        &request_id,
    )
    .await?;

    sqlx::query(
        r#"
        UPDATE inventory_goods_receipt SET
            status = 'posted', received_at = now(), journal_public_id = $3,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(grn_id)
    .bind(&journal_public_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_grn(&mut tx, org_id, grn_id, &request_id).await?;
    let dto = to_dto(&mut tx, org_id, row, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "goods_receipt",
        "posted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "purchase_order_id": dto.purchase_order_id,
            "journal_public_id": journal_public_id,
            "total_value_minor": total_value_minor,
            "po_status": new_po_status,
        }),
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
        "inventory.goods_receipt.post",
        "goods_receipt",
        &dto.id,
        serde_json::json!({ "journal_public_id": journal_public_id, "total_value_minor": total_value_minor }),
    )
    .await
    .map_err(internal(&request_id))?;

    let body_json = serde_json::to_value(&dto)
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;
    if let Some(key) = idem_key {
        idempotency::put(
            &mut *tx,
            org_id,
            "goods_receipt.post",
            &key,
            200,
            body_json.clone(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((axum::http::StatusCode::OK, Json(body_json)).into_response())
}
