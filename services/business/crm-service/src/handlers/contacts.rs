//! `/api/v1/sales/customers/{id}/contacts` and `/api/v1/sales/contacts/{id}`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::Postgres;
use uuid::Uuid;

use super::{internal, not_found, parse_public_id, validation};
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope, required_scope_for_owner_row};
use crate::state::AppState;
use crate::types::{ContactDto, ContactListResponse, CreateContactRequest, UpdateContactRequest};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/sales/customers/{id}/contacts",
            get(list_contacts).post(create_contact),
        )
        .route("/api/v1/sales/contacts/{id}", patch(update_contact))
}

#[derive(Debug, sqlx::FromRow)]
struct ContactRow {
    public_id: String,
    customer_id: Uuid,
    first_name: String,
    last_name: String,
    email: Option<String>,
    phone: Option<String>,
    title: Option<String>,
    is_primary: bool,
    owner_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ContactRow {
    fn into_dto(self) -> ContactDto {
        ContactDto {
            id: self.public_id,
            customer_id: PublicId::new(IdKind::Customer, self.customer_id).as_str(),
            first_name: self.first_name,
            last_name: self.last_name,
            email: self.email,
            phone: self.phone,
            title: self.title,
            is_primary: self.is_primary,
            owner_user_id: self.owner_user_id.map(|u| PublicId::new(IdKind::User, u).as_str()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
        }
    }
}

/// GET /api/v1/sales/customers/{id}/contacts
#[utoipa::path(get, path = "/api/v1/sales/customers/{id}/contacts", tag = "sales-contacts",
    responses((status = 200, body = ContactListResponse)))]
pub async fn list_contacts(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ContactListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let customer_id = parse_public_id(IdKind::Customer, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_contact_read(), &request_id)?;
    let scope = crate::scope::scope_for_permission(&membership.principal, &perms::sales_contact_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut qb: sqlx::QueryBuilder<Postgres> = sqlx::QueryBuilder::new(
        "SELECT public_id, customer_id, first_name, last_name, email, phone, title, is_primary, owner_user_id, created_at, updated_at FROM sales_contact WHERE org_id = ",
    );
    qb.push_bind(org_id);
    qb.push(" AND customer_id = ");
    qb.push_bind(customer_id);
    qb.push(" AND deleted_at IS NULL");
    crate::scope::push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    qb.push(" ORDER BY is_primary DESC, created_at ASC");

    let rows: Vec<ContactRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ContactListResponse {
        items: rows.into_iter().map(ContactRow::into_dto).collect(),
    }))
}

/// POST /api/v1/sales/customers/{id}/contacts
#[utoipa::path(post, path = "/api/v1/sales/customers/{id}/contacts", tag = "sales-contacts",
    request_body = CreateContactRequest,
    responses((status = 201, body = ContactDto)))]
pub async fn create_contact(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<CreateContactRequest>,
) -> Result<(StatusCode, Json<ContactDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let customer_id = parse_public_id(IdKind::Customer, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_contact_create(), &request_id)?;

    if body.first_name.trim().is_empty() && body.last_name.trim().is_empty() {
        return Err(validation(&request_id, "first_name or last_name required"));
    }

    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => Some(auth.ctx.actor.user_id),
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM sales_customer WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL")
            .bind(org_id)
            .bind(customer_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    if exists.is_none() {
        return Err(not_found(&request_id, "customer"));
    }

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Contact, id);
    let row: ContactRow = sqlx::query_as(
        r#"
        INSERT INTO sales_contact (
            id, org_id, public_id, customer_id, first_name, last_name, email, phone, title, is_primary, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        RETURNING public_id, customer_id, first_name, last_name, email, phone, title, is_primary, owner_user_id, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(customer_id)
    .bind(&body.first_name)
    .bind(&body.last_name)
    .bind(&body.email)
    .bind(&body.phone)
    .bind(&body.title)
    .bind(body.is_primary)
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(row.into_dto())))
}

/// PATCH /api/v1/sales/contacts/{id}
#[utoipa::path(patch, path = "/api/v1/sales/contacts/{id}", tag = "sales-contacts",
    request_body = UpdateContactRequest,
    responses((status = 200, body = ContactDto), (status = 404)))]
pub async fn update_contact(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateContactRequest>,
) -> Result<Json<ContactDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let contact_id = parse_public_id(IdKind::Contact, &id, &request_id)?;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_contact_update(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let current: Option<ContactRow> = sqlx::query_as(
        r#"
        SELECT public_id, customer_id, first_name, last_name, email, phone, title, is_primary, owner_user_id, created_at, updated_at
        FROM sales_contact WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(contact_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let Some(current) = current else {
        return Err(not_found(&request_id, "contact"));
    };

    let required_scope = required_scope_for_owner_row(
        &mut tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        current.owner_user_id,
    )
    .await
    .map_err(internal(&request_id))?;
    crate::principal::enforce_scoped(
        &membership.principal,
        perms::sales_contact_update(),
        required_scope,
        &request_id,
    )?;

    let first_name = body.first_name.unwrap_or(current.first_name);
    let last_name = body.last_name.unwrap_or(current.last_name);
    let email = body.email.or(current.email);
    let phone = body.phone.or(current.phone);
    let title = body.title.or(current.title);
    let is_primary = body.is_primary.unwrap_or(current.is_primary);
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => current.owner_user_id,
    };

    let row: ContactRow = sqlx::query_as(
        r#"
        UPDATE sales_contact
        SET first_name = $3, last_name = $4, email = $5, phone = $6, title = $7, is_primary = $8,
            owner_user_id = $9, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING public_id, customer_id, first_name, last_name, email, phone, title, is_primary, owner_user_id, created_at, updated_at
        "#,
    )
    .bind(org_id)
    .bind(contact_id)
    .bind(&first_name)
    .bind(&last_name)
    .bind(&email)
    .bind(&phone)
    .bind(&title)
    .bind(is_primary)
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}
