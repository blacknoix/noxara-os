//! `/api/v1/sales/territories` — territory CRUD and customer/deal assignment.

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
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{conflict, if_match_version, internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{
    enforce_any_scope, load_membership_scope_for, required_scope_for_owner_row, MembershipScope,
};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    AssignTerritoryRequest, CreateTerritoryRequest, ListQuery, TerritoryAssignmentDto,
    TerritoryDto, TerritoryListResponse, UpdateTerritoryRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/sales/territories",
            get(list_territories).post(create_territory),
        )
        .route(
            "/api/v1/sales/territories/{id}",
            get(get_territory).patch(update_territory),
        )
        .route(
            "/api/v1/sales/territories/{id}/assign",
            post(assign_territory),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct TerritoryRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    name: String,
    description: Option<String>,
    owner_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const TERRITORY_COLUMNS: &str =
    "id, public_id, name, description, owner_user_id, created_at, updated_at, version";

impl TerritoryRow {
    fn into_dto(self) -> TerritoryDto {
        TerritoryDto {
            id: self.public_id,
            name: self.name,
            description: self.description,
            owner_user_id: self
                .owner_user_id
                .map(|u| PublicId::new(IdKind::User, u).as_str()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

async fn fetch_territory_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    territory_id: Uuid,
) -> Result<Option<TerritoryRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {TERRITORY_COLUMNS} FROM sales_territory WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(territory_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn enforce_territory_scope(
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

/// GET /api/v1/sales/territories
#[utoipa::path(get, path = "/api/v1/sales/territories", tag = "sales-territories",
    responses((status = 200, body = TerritoryListResponse)))]
pub async fn list_territories(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<TerritoryListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_territory_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_territory_read());
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
        if let Some(term) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            qb.push(" AND name ILIKE ");
            qb.push_bind(format!("%{term}%"));
        }
    };

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM sales_territory WHERE org_id = ");
    count_qb.push_bind(org_id);
    build_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {TERRITORY_COLUMNS} FROM sales_territory WHERE org_id = "
    ));
    qb.push_bind(org_id);
    build_filters(&mut qb);
    qb.push(" ORDER BY name ASC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<TerritoryRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(TerritoryListResponse {
        items: rows.into_iter().map(TerritoryRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/sales/territories
#[utoipa::path(post, path = "/api/v1/sales/territories", tag = "sales-territories",
    request_body = CreateTerritoryRequest,
    responses((status = 201, body = TerritoryDto)))]
pub async fn create_territory(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateTerritoryRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_territory_manage(),
        &request_id,
    )?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name must not be empty"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "territory.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => Some(auth.ctx.actor.user_id),
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Territory, id);

    let row: TerritoryRow = sqlx::query_as(&format!(
        r#"
        INSERT INTO sales_territory (id, org_id, public_id, name, description, owner_user_id)
        VALUES ($1,$2,$3,$4,$5,$6)
        RETURNING {TERRITORY_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&body.name)
    .bind(&body.description)
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = row.into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "territory",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "name": dto.name }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "territory.create",
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

/// GET /api/v1/sales/territories/{id}
#[utoipa::path(get, path = "/api/v1/sales/territories/{id}", tag = "sales-territories",
    responses((status = 200, body = TerritoryDto), (status = 404)))]
pub async fn get_territory(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<TerritoryDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let territory_id = parse_public_id(IdKind::Territory, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_territory_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_territory_row(&mut tx, org_id, territory_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "territory"))?;
    enforce_territory_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_territory_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/sales/territories/{id}
#[utoipa::path(patch, path = "/api/v1/sales/territories/{id}", tag = "sales-territories",
    request_body = UpdateTerritoryRequest,
    responses((status = 200, body = TerritoryDto), (status = 404)))]
pub async fn update_territory(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateTerritoryRequest>,
) -> Result<Json<TerritoryDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let territory_id = parse_public_id(IdKind::Territory, &id, &request_id)?;
    let expected_version = if_match_version(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_territory_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let row = fetch_territory_row(&mut tx, org_id, territory_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "territory"))?;
    enforce_territory_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_territory_manage(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if let Some(expected) = expected_version {
        if expected != row.version {
            return Err(conflict(
                &request_id,
                format!(
                    "version mismatch: expected {expected}, current {}",
                    row.version
                ),
            ));
        }
    }

    let name = body.name.unwrap_or(row.name);
    if name.trim().is_empty() {
        return Err(validation(&request_id, "name must not be empty"));
    }
    let description = body.description.or(row.description);
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::User, s, &request_id)?),
        None => row.owner_user_id,
    };

    let updated: TerritoryRow = sqlx::query_as(&format!(
        r#"
        UPDATE sales_territory
        SET name = $3, description = $4, owner_user_id = $5, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {TERRITORY_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(territory_id)
    .bind(&name)
    .bind(&description)
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
        "sales.territory.update",
        "territory",
        &updated.public_id,
        serde_json::json!({ "name": name }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}

/// POST /api/v1/sales/territories/{id}/assign
#[utoipa::path(post, path = "/api/v1/sales/territories/{id}/assign", tag = "sales-territories",
    request_body = AssignTerritoryRequest,
    responses((status = 200, body = TerritoryAssignmentDto), (status = 403)))]
pub async fn assign_territory(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<AssignTerritoryRequest>,
) -> Result<Json<TerritoryAssignmentDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let territory_id = parse_public_id(IdKind::Territory, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_territory_manage(),
        &request_id,
    )?;

    let customer_id = super::parse_optional_public_id(
        IdKind::Customer,
        body.customer_id.as_deref(),
        &request_id,
    )?;
    let deal_id =
        super::parse_optional_public_id(IdKind::Deal, body.deal_id.as_deref(), &request_id)?;

    match (customer_id, deal_id) {
        (Some(_), None) | (None, Some(_)) => {}
        _ => {
            return Err(validation(
                &request_id,
                "exactly one of customer_id or deal_id is required",
            ));
        }
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_territory_row(&mut tx, org_id, territory_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "territory"))?;
    enforce_territory_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::sales_territory_manage(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if let Some(cid) = customer_id {
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM sales_customer WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .bind(cid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if exists.is_none() {
            return Err(not_found(&request_id, "customer"));
        }
    }
    if let Some(did) = deal_id {
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM sales_deal WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .bind(did)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if exists.is_none() {
            return Err(not_found(&request_id, "deal"));
        }
    }

    let assign_id = new_uuid_v7();
    let assigned_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO sales_territory_assignment (id, org_id, territory_id, customer_id, deal_id)
        VALUES ($1,$2,$3,$4,$5)
        RETURNING assigned_at
        "#,
    )
    .bind(assign_id)
    .bind(org_id)
    .bind(territory_id)
    .bind(customer_id)
    .bind(deal_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Some(cid) = customer_id {
        sqlx::query(
            "UPDATE sales_customer SET territory_id = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(cid)
        .bind(territory_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let dto = TerritoryAssignmentDto {
        territory_id: row.public_id.clone(),
        customer_id: customer_id.map(|u| PublicId::new(IdKind::Customer, u).as_str()),
        deal_id: deal_id.map(|u| PublicId::new(IdKind::Deal, u).as_str()),
        assigned_at: assigned_at.to_rfc3339(),
    };

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "territory",
        "assigned",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "territory_id": dto.territory_id,
            "customer_id": dto.customer_id,
            "deal_id": dto.deal_id,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "sales.territory.assign",
        "territory",
        &row.public_id,
        serde_json::json!({
            "customer_id": dto.customer_id,
            "deal_id": dto.deal_id,
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
