//! `/api/v1/inventory/warehouses` — warehouse master data CRUD.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
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
    CreateWarehouseRequest, ListQuery, UpdateWarehouseRequest, WarehouseDto, WarehouseListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/warehouses",
            get(list_warehouses).post(create_warehouse),
        )
        .route(
            "/api/v1/inventory/warehouses/{id}",
            get(get_warehouse).patch(update_warehouse),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct WarehouseRow {
    public_id: String,
    code: String,
    name: String,
    location: Option<String>,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl WarehouseRow {
    fn into_dto(self) -> WarehouseDto {
        WarehouseDto {
            id: self.public_id,
            code: self.code,
            name: self.name,
            location: self.location,
            is_active: self.is_active,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

const COLS: &str = r#"
    public_id, code, name, location, is_active, created_at, updated_at, version
"#;

/// GET /api/v1/inventory/warehouses
#[utoipa::path(get, path = "/api/v1/inventory/warehouses", tag = "inventory-warehouses",
    params(ListQuery), responses((status = 200, body = WarehouseListResponse)))]
pub async fn list_warehouses(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<WarehouseListResponse>, AppError> {
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
    let perm = perms::inventory_warehouse_read();
    enforce_any_scope(&membership.principal, perm.clone(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perm);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM inventory_warehouse WHERE org_id = ");
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

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {COLS} FROM inventory_warehouse WHERE org_id = "
    ));
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
    qb.push(" ORDER BY code LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<WarehouseRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(WarehouseListResponse {
        items: rows.into_iter().map(WarehouseRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/inventory/warehouses
#[utoipa::path(post, path = "/api/v1/inventory/warehouses", tag = "inventory-warehouses",
    request_body = CreateWarehouseRequest, responses((status = 201, body = WarehouseDto)))]
pub async fn create_warehouse(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateWarehouseRequest>,
) -> Result<(axum::http::StatusCode, Json<WarehouseDto>), AppError> {
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
        perms::inventory_warehouse_write(),
        &request_id,
    )?;

    if body.code.trim().is_empty() || body.name.trim().is_empty() {
        return Err(validation(&request_id, "code and name are required"));
    }

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Warehouse, id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    sqlx::query(
        r#"
        INSERT INTO inventory_warehouse (id, org_id, public_id, code, name, location, owner_user_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.code.trim())
    .bind(body.name.trim())
    .bind(body.location.as_deref())
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row: WarehouseRow = sqlx::query_as(&format!(
        "SELECT {COLS} FROM inventory_warehouse WHERE org_id = $1 AND id = $2"
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
        "warehouse",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "code": dto.code }),
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
        "inventory.warehouse.create",
        "warehouse",
        &dto.id,
        serde_json::json!({ "code": dto.code }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((axum::http::StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/inventory/warehouses/{id}
#[utoipa::path(get, path = "/api/v1/inventory/warehouses/{id}", tag = "inventory-warehouses",
    params(("id" = String, Path)), responses((status = 200, body = WarehouseDto), (status = 404)))]
pub async fn get_warehouse(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<WarehouseDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let warehouse_id = parse_public_id(IdKind::Warehouse, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_warehouse_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row: WarehouseRow = sqlx::query_as(&format!(
        "SELECT {COLS} FROM inventory_warehouse WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(warehouse_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .ok_or_else(|| not_found(&request_id, "warehouse"))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/inventory/warehouses/{id}
#[utoipa::path(patch, path = "/api/v1/inventory/warehouses/{id}", tag = "inventory-warehouses",
    params(("id" = String, Path)), request_body = UpdateWarehouseRequest,
    responses((status = 200, body = WarehouseDto), (status = 404)))]
pub async fn update_warehouse(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateWarehouseRequest>,
) -> Result<Json<WarehouseDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let warehouse_id = parse_public_id(IdKind::Warehouse, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_warehouse_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM inventory_warehouse WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .bind(warehouse_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    if existing.is_none() {
        return Err(not_found(&request_id, "warehouse"));
    }
    if let Some(ref name) = body.name {
        if name.trim().is_empty() {
            return Err(validation(&request_id, "name cannot be empty"));
        }
    }

    let row: WarehouseRow = sqlx::query_as(&format!(
        r#"
        UPDATE inventory_warehouse SET
            name = COALESCE($3, name),
            location = COALESCE($4, location),
            is_active = COALESCE($5, is_active),
            version = version + 1,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {COLS}
        "#
    ))
    .bind(org_id)
    .bind(warehouse_id)
    .bind(body.name.as_deref())
    .bind(body.location.as_deref())
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
        "inventory.warehouse.update",
        "warehouse",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
