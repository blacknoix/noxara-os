//! `/api/v1/inventory/items` — item (SKU) master data CRUD + stock levels view.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{internal, normalize_paging, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreateInventoryItemRequest, InventoryItemDto, InventoryItemListResponse, ListQuery,
    StockLevelDto, StockLevelListResponse, UpdateInventoryItemRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/items",
            get(list_items).post(create_item),
        )
        .route(
            "/api/v1/inventory/items/{id}",
            get(get_item).patch(update_item),
        )
        .route("/api/v1/inventory/items/{id}/stock", get(get_item_stock))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ItemRow {
    public_id: String,
    sku: String,
    name: String,
    description: Option<String>,
    uom: String,
    currency: String,
    reorder_point_qty: i64,
    allow_negative_stock: bool,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl ItemRow {
    fn into_dto(self) -> InventoryItemDto {
        InventoryItemDto {
            id: self.public_id,
            sku: self.sku,
            name: self.name,
            description: self.description,
            uom: self.uom,
            currency: self.currency,
            reorder_point_qty: self.reorder_point_qty,
            allow_negative_stock: self.allow_negative_stock,
            is_active: self.is_active,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

const COLS: &str = r#"
    public_id, sku, name, description, uom, currency, reorder_point_qty,
    allow_negative_stock, is_active, created_at, updated_at, version
"#;

/// GET /api/v1/inventory/items
#[utoipa::path(get, path = "/api/v1/inventory/items", tag = "inventory-items",
    params(ListQuery), responses((status = 200, body = InventoryItemListResponse)))]
pub async fn list_items(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<InventoryItemListResponse>, AppError> {
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
    let perm = perms::inventory_item_read();
    enforce_any_scope(&membership.principal, perm.clone(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perm);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM inventory_item WHERE org_id = ");
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
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new(format!("SELECT {COLS} FROM inventory_item WHERE org_id = "));
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
    qb.push(" ORDER BY sku LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<ItemRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(InventoryItemListResponse {
        items: rows.into_iter().map(ItemRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/inventory/items
#[utoipa::path(post, path = "/api/v1/inventory/items", tag = "inventory-items",
    request_body = CreateInventoryItemRequest, responses((status = 201, body = InventoryItemDto)))]
pub async fn create_item(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateInventoryItemRequest>,
) -> Result<(axum::http::StatusCode, Json<InventoryItemDto>), AppError> {
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
        perms::inventory_item_write(),
        &request_id,
    )?;

    if body.sku.trim().is_empty() || body.name.trim().is_empty() {
        return Err(validation(&request_id, "sku and name are required"));
    }
    let currency: Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;
    let reorder_point_qty = body.reorder_point_qty.unwrap_or(0).max(0);

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::InventoryItem, id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    sqlx::query(
        r#"
        INSERT INTO inventory_item (
            id, org_id, public_id, sku, name, description, uom, currency,
            reorder_point_qty, allow_negative_stock, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.sku.trim())
    .bind(body.name.trim())
    .bind(body.description.as_deref())
    .bind(body.uom.as_deref().unwrap_or("each"))
    .bind(currency.as_str())
    .bind(reorder_point_qty)
    .bind(body.allow_negative_stock.unwrap_or(false))
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row: ItemRow = sqlx::query_as(&format!(
        "SELECT {COLS} FROM inventory_item WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let dto = row.into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "item",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "sku": dto.sku }),
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
        "inventory.item.create",
        "item",
        &dto.id,
        serde_json::json!({ "sku": dto.sku }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((axum::http::StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/inventory/items/{id}
#[utoipa::path(get, path = "/api/v1/inventory/items/{id}", tag = "inventory-items",
    params(("id" = String, Path)), responses((status = 200, body = InventoryItemDto), (status = 404)))]
pub async fn get_item(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<InventoryItemDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let item_id = parse_public_id(IdKind::InventoryItem, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_item_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row: ItemRow = sqlx::query_as(&format!(
        "SELECT {COLS} FROM inventory_item WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(item_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .ok_or_else(|| not_found(&request_id, "item"))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/inventory/items/{id}
#[utoipa::path(patch, path = "/api/v1/inventory/items/{id}", tag = "inventory-items",
    params(("id" = String, Path)), request_body = UpdateInventoryItemRequest,
    responses((status = 200, body = InventoryItemDto), (status = 404)))]
pub async fn update_item(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateInventoryItemRequest>,
) -> Result<Json<InventoryItemDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let item_id = parse_public_id(IdKind::InventoryItem, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_item_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM inventory_item WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .bind(item_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    if existing.is_none() {
        return Err(not_found(&request_id, "item"));
    }
    if let Some(qty) = body.reorder_point_qty {
        if qty < 0 {
            return Err(validation(&request_id, "reorder_point_qty must be >= 0"));
        }
    }

    let row: ItemRow = sqlx::query_as(&format!(
        r#"
        UPDATE inventory_item SET
            name = COALESCE($3, name),
            description = COALESCE($4, description),
            reorder_point_qty = COALESCE($5, reorder_point_qty),
            allow_negative_stock = COALESCE($6, allow_negative_stock),
            is_active = COALESCE($7, is_active),
            version = version + 1,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {COLS}
        "#
    ))
    .bind(org_id)
    .bind(item_id)
    .bind(body.name.as_deref())
    .bind(body.description.as_deref())
    .bind(body.reorder_point_qty)
    .bind(body.allow_negative_stock)
    .bind(body.is_active)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let dto = row.into_dto();

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.item.update",
        "item",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// GET /api/v1/inventory/items/{id}/stock
#[utoipa::path(get, path = "/api/v1/inventory/items/{id}/stock", tag = "inventory-items",
    params(("id" = String, Path)), responses((status = 200, body = StockLevelListResponse)))]
pub async fn get_item_stock(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<StockLevelListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let item_id = parse_public_id(IdKind::InventoryItem, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_stock_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    #[derive(sqlx::FromRow)]
    struct LevelRow {
        warehouse_public_id: String,
        item_public_id: String,
        qty_on_hand: i64,
        avg_unit_cost_minor: i64,
        last_movement_at: Option<DateTime<Utc>>,
        updated_at: DateTime<Utc>,
    }

    let rows: Vec<LevelRow> = sqlx::query_as(
        r#"
        SELECT w.public_id AS warehouse_public_id, i.public_id AS item_public_id,
               s.qty_on_hand, s.avg_unit_cost_minor, s.last_movement_at, s.updated_at
        FROM inventory_stock_level s
        JOIN inventory_warehouse w ON w.id = s.warehouse_id AND w.org_id = s.org_id
        JOIN inventory_item i ON i.id = s.item_id AND i.org_id = s.org_id
        WHERE s.org_id = $1 AND s.item_id = $2
        ORDER BY w.code
        "#,
    )
    .bind(org_id)
    .bind(item_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(StockLevelListResponse {
        items: rows
            .into_iter()
            .map(|r| StockLevelDto {
                warehouse_id: r.warehouse_public_id,
                item_id: r.item_public_id,
                qty_on_hand: r.qty_on_hand,
                avg_unit_cost_minor: r.avg_unit_cost_minor,
                last_movement_at: r.last_movement_at.map(|t| t.to_rfc3339()),
                updated_at: r.updated_at.to_rfc3339(),
            })
            .collect(),
    }))
}
