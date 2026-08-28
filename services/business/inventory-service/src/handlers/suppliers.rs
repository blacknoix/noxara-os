//! `/api/v1/inventory/suppliers` — supplier master data CRUD.

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
    CreateSupplierRequest, ListQuery, SupplierDto, SupplierListResponse, UpdateSupplierRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/suppliers",
            get(list_suppliers).post(create_supplier),
        )
        .route(
            "/api/v1/inventory/suppliers/{id}",
            get(get_supplier).patch(update_supplier),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct SupplierRow {
    public_id: String,
    name: String,
    email: Option<String>,
    phone: Option<String>,
    currency: String,
    payment_terms: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl SupplierRow {
    fn into_dto(self) -> SupplierDto {
        SupplierDto {
            id: self.public_id,
            name: self.name,
            email: self.email,
            phone: self.phone,
            currency: self.currency,
            payment_terms: self.payment_terms,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

const COLS: &str = r#"
    public_id, name, email, phone, currency, payment_terms, created_at, updated_at, version
"#;

/// GET /api/v1/inventory/suppliers
#[utoipa::path(get, path = "/api/v1/inventory/suppliers", tag = "inventory-suppliers",
    params(ListQuery), responses((status = 200, body = SupplierListResponse)))]
pub async fn list_suppliers(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<SupplierListResponse>, AppError> {
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
    let perm = perms::inventory_supplier_read();
    enforce_any_scope(&membership.principal, perm.clone(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perm);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM inventory_supplier WHERE org_id = ");
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
        QueryBuilder::new(format!("SELECT {COLS} FROM inventory_supplier WHERE org_id = "));
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
    qb.push(" ORDER BY name LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<SupplierRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(SupplierListResponse {
        items: rows.into_iter().map(SupplierRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/inventory/suppliers
#[utoipa::path(post, path = "/api/v1/inventory/suppliers", tag = "inventory-suppliers",
    request_body = CreateSupplierRequest, responses((status = 201, body = SupplierDto)))]
pub async fn create_supplier(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateSupplierRequest>,
) -> Result<(axum::http::StatusCode, Json<SupplierDto>), AppError> {
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
        perms::inventory_supplier_write(),
        &request_id,
    )?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name is required"));
    }
    let currency: Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Supplier, id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    sqlx::query(
        r#"
        INSERT INTO inventory_supplier (id, org_id, public_id, name, email, phone, currency, payment_terms, owner_user_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(body.email.as_deref())
    .bind(body.phone.as_deref())
    .bind(currency.as_str())
    .bind(body.payment_terms.as_deref())
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row: SupplierRow = sqlx::query_as(&format!(
        "SELECT {COLS} FROM inventory_supplier WHERE org_id = $1 AND id = $2"
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
        "supplier",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "name": dto.name }),
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
        "inventory.supplier.create",
        "supplier",
        &dto.id,
        serde_json::json!({ "name": dto.name }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((axum::http::StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/inventory/suppliers/{id}
#[utoipa::path(get, path = "/api/v1/inventory/suppliers/{id}", tag = "inventory-suppliers",
    params(("id" = String, Path)), responses((status = 200, body = SupplierDto), (status = 404)))]
pub async fn get_supplier(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<SupplierDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let supplier_id = parse_public_id(IdKind::Supplier, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_supplier_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row: SupplierRow = sqlx::query_as(&format!(
        "SELECT {COLS} FROM inventory_supplier WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(supplier_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .ok_or_else(|| not_found(&request_id, "supplier"))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/inventory/suppliers/{id}
#[utoipa::path(patch, path = "/api/v1/inventory/suppliers/{id}", tag = "inventory-suppliers",
    params(("id" = String, Path)), request_body = UpdateSupplierRequest,
    responses((status = 200, body = SupplierDto), (status = 404)))]
pub async fn update_supplier(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateSupplierRequest>,
) -> Result<Json<SupplierDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let supplier_id = parse_public_id(IdKind::Supplier, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_supplier_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM inventory_supplier WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .bind(supplier_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    if existing.is_none() {
        return Err(not_found(&request_id, "supplier"));
    }

    let row: SupplierRow = sqlx::query_as(&format!(
        r#"
        UPDATE inventory_supplier SET
            name = COALESCE($3, name),
            email = COALESCE($4, email),
            phone = COALESCE($5, phone),
            payment_terms = COALESCE($6, payment_terms),
            version = version + 1,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {COLS}
        "#
    ))
    .bind(org_id)
    .bind(supplier_id)
    .bind(body.name.as_deref())
    .bind(body.email.as_deref())
    .bind(body.phone.as_deref())
    .bind(body.payment_terms.as_deref())
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
        "inventory.supplier.update",
        "supplier",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
