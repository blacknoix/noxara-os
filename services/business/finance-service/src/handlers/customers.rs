//! `/api/v1/finance/customers` — projected sales customers + outstanding AR.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::IdKind;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};

use super::{internal, normalize_paging, not_found, parse_public_id};
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{FinanceCustomerDto, FinanceCustomerListResponse, ListQuery};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/finance/customers", get(list_customers))
        .route("/api/v1/finance/customers/{id}", get(get_customer))
}

#[derive(Debug, sqlx::FromRow)]
struct CustomerRow {
    public_id: String,
    sales_customer_public_id: String,
    name: String,
    email: Option<String>,
    currency: String,
    outstanding_balance_minor: i64,
}

impl CustomerRow {
    fn into_dto(self) -> FinanceCustomerDto {
        FinanceCustomerDto {
            id: self.public_id,
            sales_customer_id: self.sales_customer_public_id,
            name: self.name,
            email: self.email,
            currency: self.currency,
            outstanding_balance_minor: self.outstanding_balance_minor,
        }
    }
}

/// GET /api/v1/finance/customers
#[utoipa::path(get, path = "/api/v1/finance/customers", tag = "finance-customers",
    params(
        ("q" = Option<String>, Query, description = "Search name/email"),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = FinanceCustomerListResponse)))]
pub async fn list_customers(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<FinanceCustomerListResponse>, AppError> {
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
        perms::finance_customer_read(),
        &request_id,
    )?;

    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM finance_customer WHERE org_id = ");
    count_qb.push_bind(org_id);
    if let Some(q) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = format!("%{q}%");
        count_qb.push(" AND (name ILIKE ");
        count_qb.push_bind(pattern.clone());
        count_qb.push(" OR email ILIKE ");
        count_qb.push_bind(pattern);
        count_qb.push(")");
    }
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        r#"
        SELECT c.public_id, c.sales_customer_public_id, c.name, c.email, c.currency,
               COALESCE((
                   SELECT SUM(i.balance_minor) FROM finance_invoice i
                   WHERE i.org_id = c.org_id AND i.customer_id = c.id
                     AND i.status NOT IN ('draft', 'void')
               ), 0)::BIGINT AS outstanding_balance_minor
        FROM finance_customer c
        WHERE c.org_id =
        "#,
    );
    qb.push_bind(org_id);
    if let Some(q) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = format!("%{q}%");
        qb.push(" AND (c.name ILIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR c.email ILIKE ");
        qb.push_bind(pattern);
        qb.push(")");
    }
    qb.push(" ORDER BY c.name ASC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<CustomerRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(FinanceCustomerListResponse {
        items: rows.into_iter().map(CustomerRow::into_dto).collect(),
        total,
    }))
}

/// GET /api/v1/finance/customers/{id}
#[utoipa::path(get, path = "/api/v1/finance/customers/{id}", tag = "finance-customers",
    responses((status = 200, body = FinanceCustomerDto), (status = 404)))]
pub async fn get_customer(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<FinanceCustomerDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let _ = parse_public_id(IdKind::Customer, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_customer_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<CustomerRow> = sqlx::query_as(
        r#"
        SELECT c.public_id, c.sales_customer_public_id, c.name, c.email, c.currency,
               COALESCE((
                   SELECT SUM(i.balance_minor) FROM finance_invoice i
                   WHERE i.org_id = c.org_id AND i.customer_id = c.id
                     AND i.status NOT IN ('draft', 'void')
               ), 0)::BIGINT AS outstanding_balance_minor
        FROM finance_customer c
        WHERE c.org_id = $1 AND c.public_id = $2
        "#,
    )
    .bind(org_id)
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    let row = row.ok_or_else(|| not_found(&request_id, "customer"))?;
    Ok(Json(row.into_dto()))
}
