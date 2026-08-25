//! `/api/v1/sales/products` — quotable product/SKU catalogue.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::Postgres;

use super::{internal, not_found, parse_public_id, validation};
use crate::auth::AuthCtx;
use crate::principal::{enforce, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreateProductRequest, ListQuery, ProductDto, ProductListResponse, UpdateProductRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/sales/products",
            get(list_products).post(create_product),
        )
        .route("/api/v1/sales/products/{id}", patch(update_product))
}

#[derive(Debug, sqlx::FromRow)]
struct ProductRow {
    public_id: String,
    name: String,
    sku: Option<String>,
    unit_price_minor: Option<i64>,
    currency: Option<String>,
    tax_group: Option<String>,
    active: bool,
}

const PRODUCT_COLUMNS: &str = "public_id, name, sku, unit_price_minor, currency, tax_group, active";

impl ProductRow {
    fn into_dto(self) -> ProductDto {
        ProductDto {
            id: self.public_id,
            name: self.name,
            sku: self.sku,
            unit_price_minor: self.unit_price_minor,
            currency: self.currency,
            tax_group: self.tax_group,
            active: self.active,
        }
    }
}

/// GET /api/v1/sales/products
#[utoipa::path(get, path = "/api/v1/sales/products", tag = "sales-products",
    responses((status = 200, body = ProductListResponse)))]
pub async fn list_products(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<ProductListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce(
        &membership.principal,
        perms::sales_product_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| {
            AppError::new(
                companyos_errors::ErrorCode::Internal,
                request_id.clone(),
                e.to_string(),
            )
        })?;

    let mut qb: sqlx::QueryBuilder<Postgres> = sqlx::QueryBuilder::new(format!(
        "SELECT {PRODUCT_COLUMNS} FROM sales_product WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    if let Some(term) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = format!("%{term}%");
        qb.push(" AND (name ILIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR sku ILIKE ");
        qb.push_bind(pattern);
        qb.push(")");
    }
    qb.push(" ORDER BY name ASC");

    let rows: Vec<ProductRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ProductListResponse {
        items: rows.into_iter().map(ProductRow::into_dto).collect(),
    }))
}

/// POST /api/v1/sales/products
#[utoipa::path(post, path = "/api/v1/sales/products", tag = "sales-products",
    request_body = CreateProductRequest,
    responses((status = 201, body = ProductDto)))]
pub async fn create_product(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateProductRequest>,
) -> Result<(StatusCode, Json<ProductDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce(
        &membership.principal,
        perms::sales_product_manage(),
        &request_id,
    )?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name must not be empty"));
    }
    if let Some(c) = body.currency.as_deref() {
        companyos_money::Currency::new(c)
            .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    }

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Product, id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| {
            AppError::new(
                companyos_errors::ErrorCode::Internal,
                request_id.clone(),
                e.to_string(),
            )
        })?;

    let row: ProductRow = sqlx::query_as(&format!(
        r#"
        INSERT INTO sales_product (id, org_id, public_id, name, sku, unit_price_minor, currency, tax_group, active)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING {PRODUCT_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&body.name)
    .bind(&body.sku)
    .bind(body.unit_price_minor)
    .bind(&body.currency)
    .bind(&body.tax_group)
    .bind(body.active)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(row.into_dto())))
}

/// PATCH /api/v1/sales/products/{id}
#[utoipa::path(patch, path = "/api/v1/sales/products/{id}", tag = "sales-products",
    request_body = UpdateProductRequest,
    responses((status = 200, body = ProductDto), (status = 404)))]
pub async fn update_product(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateProductRequest>,
) -> Result<Json<ProductDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let product_id = parse_public_id(IdKind::Product, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce(
        &membership.principal,
        perms::sales_product_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| {
            AppError::new(
                companyos_errors::ErrorCode::Internal,
                request_id.clone(),
                e.to_string(),
            )
        })?;

    let current: Option<ProductRow> = sqlx::query_as(&format!(
        "SELECT {PRODUCT_COLUMNS} FROM sales_product WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(product_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let Some(current) = current else {
        return Err(not_found(&request_id, "product"));
    };

    let name = body.name.unwrap_or(current.name);
    let sku = body.sku.or(current.sku);
    let unit_price_minor = body.unit_price_minor.or(current.unit_price_minor);
    let currency = body.currency.or(current.currency);
    let tax_group = body.tax_group.or(current.tax_group);
    let active = body.active.unwrap_or(current.active);

    let row: ProductRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_product
        SET name = $3, sku = $4, unit_price_minor = $5, currency = $6, tax_group = $7, active = $8, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {PRODUCT_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(product_id)
    .bind(&name)
    .bind(&sku)
    .bind(unit_price_minor)
    .bind(&currency)
    .bind(&tax_group)
    .bind(active)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}
