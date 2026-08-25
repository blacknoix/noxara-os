//! `/api/v1/sales/customers` — customer accounts.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
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

use super::{conflict, if_match_version, internal, is_unique_violation, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::dupes::find_customer_duplicates;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreateCustomerRequest, CreateCustomerResponse, CustomerDto, CustomerListResponse, ListQuery,
    UpdateCustomerRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sales/customers", get(list_customers).post(create_customer))
        .route(
            "/api/v1/sales/customers/{id}",
            get(get_customer).patch(update_customer).delete(delete_customer),
        )
}

#[derive(Debug, sqlx::FromRow)]
struct CustomerRow {
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

impl CustomerRow {
    fn into_dto(self) -> CustomerDto {
        CustomerDto {
            id: self.public_id,
            name: self.name,
            email: self.email,
            phone: self.phone,
            website: self.website,
            billing_address: self.billing_address,
            notes: self.notes,
            owner_user_id: self.owner_user_id.map(|u| PublicId::new(IdKind::User, u).as_str()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
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
        let pattern = format!("%{q}%");
        qb.push(" AND (name ILIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR email ILIKE ");
        qb.push_bind(pattern);
        qb.push(")");
    }
}

/// GET /api/v1/sales/customers
#[utoipa::path(get, path = "/api/v1/sales/customers", tag = "sales-customers",
    params(("q" = Option<String>, Query, description = "Search name/email"), ("limit" = Option<i64>, Query), ("offset" = Option<i64>, Query)),
    responses((status = 200, body = CustomerListResponse)))]
pub async fn list_customers(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<CustomerListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(&membership.principal, perms::sales_customer_read(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_customer_read());

    let (limit, offset) = super::normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM sales_customer WHERE org_id = ");
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

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "SELECT public_id, name, email, phone, website, billing_address, notes, owner_user_id, created_at, updated_at, version FROM sales_customer WHERE org_id = ",
    );
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

    let rows: Vec<CustomerRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(CustomerListResponse {
        items: rows.into_iter().map(CustomerRow::into_dto).collect(),
        total,
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateCustomerQuery {
    #[serde(default)]
    pub strict: bool,
}

/// POST /api/v1/sales/customers
#[utoipa::path(post, path = "/api/v1/sales/customers", tag = "sales-customers",
    request_body = CreateCustomerRequest,
    responses((status = 201, body = CreateCustomerResponse), (status = 409, description = "Strict duplicate check failed")))]
pub async fn create_customer(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<CreateCustomerQuery>,
    Json(body): Json<CreateCustomerRequest>,
) -> Result<(StatusCode, Json<CreateCustomerResponse>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_customer_create(), &request_id)?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name must not be empty"));
    }

    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => super::parse_public_id(companyos_ids::IdKind::User, s, &request_id)?,
        None => auth.ctx.actor.user_id,
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Customer, id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let duplicates = find_customer_duplicates(&mut tx, org_id, &body.name, body.email.as_deref())
        .await
        .map_err(internal(&request_id))?;

    if q.strict && !duplicates.is_empty() {
        return Err(conflict(
            &request_id,
            format!(
                "found {} potential duplicate customer(s); retry without strict=true to create anyway",
                duplicates.len()
            ),
        ));
    }

    sqlx::query(
        r#"
        INSERT INTO sales_customer (
            id, org_id, public_id, name, email, phone, website, billing_address, notes, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&body.name)
    .bind(&body.email)
    .bind(&body.phone)
    .bind(&body.website)
    .bind(&body.billing_address)
    .bind(&body.notes)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        // `sales_customer_org_email_unique_idx` enforces one active customer per
        // (org, email); non-strict callers still hit it if they reuse an email —
        // surface a clean 409 instead of an opaque 500.
        if is_unique_violation(&e, "sales_customer_org_email_unique_idx") {
            conflict(&request_id, "a customer with this email already exists")
        } else {
            internal(&request_id)(e)
        }
    })?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "customer",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": public_id.as_str(), "name": body.name }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "sales.customer.create",
        "customer",
        &public_id.as_str(),
        serde_json::json!({ "name": body.name }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    let dto = CustomerDto {
        id: public_id.as_str(),
        name: body.name,
        email: body.email,
        phone: body.phone,
        website: body.website,
        billing_address: body.billing_address,
        notes: body.notes,
        owner_user_id: Some(PublicId::new(IdKind::User, owner_user_id).as_str()),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
        version: 1,
    };

    Ok((
        StatusCode::CREATED,
        Json(CreateCustomerResponse {
            customer: dto,
            duplicate_warnings: duplicates,
        }),
    ))
}

async fn fetch_customer_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    customer_id: Uuid,
) -> Result<Option<CustomerRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT public_id, name, email, phone, website, billing_address, notes, owner_user_id, created_at, updated_at, version
        FROM sales_customer
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(customer_id)
    .fetch_optional(&mut **tx)
    .await
}

/// GET /api/v1/sales/customers/{id}
#[utoipa::path(get, path = "/api/v1/sales/customers/{id}", tag = "sales-customers",
    responses((status = 200, body = CustomerDto), (status = 404)))]
pub async fn get_customer(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<CustomerDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let customer_id = parse_public_id(IdKind::Customer, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_customer_read(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_customer_row(&mut tx, org_id, customer_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "customer"))?;

    let required_scope = crate::principal::required_scope_for_owner_row(
        &mut tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        row.owner_user_id,
    )
    .await
    .map_err(internal(&request_id))?;
    crate::principal::enforce_scoped(
        &membership.principal,
        perms::sales_customer_read(),
        required_scope,
        &request_id,
    )?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/sales/customers/{id}
#[utoipa::path(patch, path = "/api/v1/sales/customers/{id}", tag = "sales-customers",
    request_body = UpdateCustomerRequest,
    responses((status = 200, body = CustomerDto), (status = 404), (status = 409, description = "version mismatch")))]
pub async fn update_customer(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateCustomerRequest>,
) -> Result<Json<CustomerDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let customer_id = parse_public_id(IdKind::Customer, &id, &request_id)?;
    let expected_version = if_match_version(&headers);

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_customer_update(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_customer_row(&mut tx, org_id, customer_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "customer"))?;

    let required_scope = crate::principal::required_scope_for_owner_row(
        &mut tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        row.owner_user_id,
    )
    .await
    .map_err(internal(&request_id))?;
    crate::principal::enforce_scoped(
        &membership.principal,
        perms::sales_customer_update(),
        required_scope,
        &request_id,
    )?;

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
    let website = body.website.or(row.website);
    let billing_address = body.billing_address.or(row.billing_address);
    let notes = body.notes.or(row.notes);
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => row.owner_user_id,
    };

    let updated: CustomerRow = sqlx::query_as(
        r#"
        UPDATE sales_customer
        SET name = $3, email = $4, phone = $5, website = $6, billing_address = $7, notes = $8,
            owner_user_id = $9, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING public_id, name, email, phone, website, billing_address, notes, owner_user_id, created_at, updated_at, version
        "#,
    )
    .bind(org_id)
    .bind(customer_id)
    .bind(&name)
    .bind(&email)
    .bind(&phone)
    .bind(&website)
    .bind(&billing_address)
    .bind(&notes)
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "sales.customer.update",
        "customer",
        &updated.public_id,
        serde_json::json!({ "name": name }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}

/// DELETE /api/v1/sales/customers/{id} — soft delete.
#[utoipa::path(delete, path = "/api/v1/sales/customers/{id}", tag = "sales-customers",
    responses((status = 204), (status = 404)))]
pub async fn delete_customer(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let customer_id = parse_public_id(IdKind::Customer, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_customer_delete(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_customer_row(&mut tx, org_id, customer_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "customer"))?;

    let required_scope = crate::principal::required_scope_for_owner_row(
        &mut tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        row.owner_user_id,
    )
    .await
    .map_err(internal(&request_id))?;
    crate::principal::enforce_scoped(
        &membership.principal,
        perms::sales_customer_delete(),
        required_scope,
        &request_id,
    )?;

    sqlx::query("UPDATE sales_customer SET deleted_at = now(), updated_at = now() WHERE org_id = $1 AND id = $2")
        .bind(org_id)
        .bind(customer_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "sales.customer.delete",
        "customer",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(StatusCode::NO_CONTENT)
}
