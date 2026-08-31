//! `/api/v1/finance/entities` — multi-entity foundations (not consolidation).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreateFinanceEntityRequest, FinanceEntityDto, FinanceEntityListResponse,
    UpdateFinanceEntityRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/entities",
            get(list_entities).post(create_entity),
        )
        .route(
            "/api/v1/finance/entities/{id}",
            get(get_entity).patch(update_entity),
        )
}

#[derive(Debug, sqlx::FromRow)]
struct EntityRow {
    public_id: String,
    name: String,
    code: String,
    currency: String,
    is_default: bool,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
}

fn row_to_dto(row: EntityRow) -> FinanceEntityDto {
    FinanceEntityDto {
        id: row.public_id,
        name: row.name,
        code: row.code,
        currency: row.currency,
        is_default: row.is_default,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    }
}

/// Ensure the org has a default finance entity; create one if missing.
pub async fn ensure_default_entity(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<(Uuid, String), sqlx::Error> {
    let existing: Option<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, public_id FROM finance_entity
        WHERE org_id = $1 AND is_default = true
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(row) = existing {
        return Ok(row);
    }

    let public_id = PublicId::generate(IdKind::FinanceEntity);
    let id = public_id.uuid();
    sqlx::query(
        r#"
        INSERT INTO finance_entity (
            id, org_id, public_id, name, code, currency, is_default
        ) VALUES ($1,$2,$3,'Default','DEFAULT','USD',true)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .execute(&mut **tx)
    .await?;
    Ok((id, public_id.as_str().to_string()))
}

/// Resolve entity uuid from optional public id, falling back to org default.
pub async fn resolve_entity_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    entity_public_id: Option<&str>,
    request_id: &str,
) -> Result<(Uuid, String), AppError> {
    if let Some(raw) = entity_public_id.filter(|s| !s.trim().is_empty()) {
        let uuid = parse_public_id(IdKind::FinanceEntity, raw, request_id)?;
        let row: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT id, public_id FROM finance_entity WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(uuid)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal(request_id))?;
        return row.ok_or_else(|| not_found(request_id, "entity"));
    }
    ensure_default_entity(tx, org_id)
        .await
        .map_err(internal(request_id))
}

/// GET /api/v1/finance/entities
#[utoipa::path(get, path = "/api/v1/finance/entities", tag = "finance-entities",
    responses((status = 200, body = FinanceEntityListResponse)))]
pub async fn list_entities(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<FinanceEntityListResponse>, AppError> {
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
        perms::finance_entity_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_default_entity(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let rows: Vec<EntityRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, code, currency, is_default, created_at, updated_at
        FROM finance_entity WHERE org_id = $1 ORDER BY code ASC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let total = rows.len() as i64;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(FinanceEntityListResponse {
        items: rows.into_iter().map(row_to_dto).collect(),
        total,
    }))
}

/// POST /api/v1/finance/entities
#[utoipa::path(post, path = "/api/v1/finance/entities", tag = "finance-entities",
    request_body = CreateFinanceEntityRequest,
    responses((status = 201, body = FinanceEntityDto)))]
pub async fn create_entity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateFinanceEntityRequest>,
) -> Result<impl IntoResponse, AppError> {
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
        perms::finance_entity_manage(),
        &request_id,
    )?;

    if body.name.trim().is_empty() || body.code.trim().is_empty() {
        return Err(validation(&request_id, "name and code are required"));
    }
    let _currency = Currency::new(&body.currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;

    let public_id = PublicId::generate(IdKind::FinanceEntity);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if body.is_default {
        sqlx::query("UPDATE finance_entity SET is_default = false WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    }

    sqlx::query(
        r#"
        INSERT INTO finance_entity (
            id, org_id, public_id, name, code, currency, is_default
        ) VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(body.code.trim().to_uppercase())
    .bind(&body.currency)
    .bind(body.is_default)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.entity.create",
        "entity",
        &public_id.as_str(),
        serde_json::json!({ "code": body.code.trim() }),
    )
    .await
    .map_err(internal(&request_id))?;

    let row: EntityRow = sqlx::query_as(
        r#"
        SELECT public_id, name, code, currency, is_default, created_at, updated_at
        FROM finance_entity WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(row_to_dto(row))))
}

/// GET /api/v1/finance/entities/{id}
#[utoipa::path(get, path = "/api/v1/finance/entities/{id}", tag = "finance-entities",
    responses((status = 200, body = FinanceEntityDto)))]
pub async fn get_entity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<FinanceEntityDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let entity_id = parse_public_id(IdKind::FinanceEntity, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_entity_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<EntityRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, code, currency, is_default, created_at, updated_at
        FROM finance_entity WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(entity_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row_to_dto(
        row.ok_or_else(|| not_found(&request_id, "entity"))?,
    )))
}

/// PATCH /api/v1/finance/entities/{id}
#[utoipa::path(patch, path = "/api/v1/finance/entities/{id}", tag = "finance-entities",
    request_body = UpdateFinanceEntityRequest,
    responses((status = 200, body = FinanceEntityDto)))]
pub async fn update_entity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateFinanceEntityRequest>,
) -> Result<Json<FinanceEntityDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let entity_id = parse_public_id(IdKind::FinanceEntity, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_entity_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<EntityRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, code, currency, is_default, created_at, updated_at
        FROM finance_entity WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(entity_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let row = row.ok_or_else(|| not_found(&request_id, "entity"))?;

    if let Some(ref c) = body.currency {
        let _ = Currency::new(c).map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    }

    if body.is_default == Some(true) {
        sqlx::query("UPDATE finance_entity SET is_default = false WHERE org_id = $1 AND id <> $2")
            .bind(org_id)
            .bind(entity_id)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    }

    let name = body.name.as_deref().unwrap_or(&row.name);
    let code = body
        .code
        .as_deref()
        .map(|c| c.trim().to_uppercase())
        .unwrap_or_else(|| row.code.clone());
    let currency = body.currency.as_deref().unwrap_or(&row.currency);
    let is_default = body.is_default.unwrap_or(row.is_default);

    sqlx::query(
        r#"
        UPDATE finance_entity SET
            name = $3, code = $4, currency = $5, is_default = $6, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(entity_id)
    .bind(name)
    .bind(&code)
    .bind(currency)
    .bind(is_default)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.entity.update",
        "entity",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let updated: EntityRow = sqlx::query_as(
        r#"
        SELECT public_id, name, code, currency, is_default, created_at, updated_at
        FROM finance_entity WHERE id = $1
        "#,
    )
    .bind(entity_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row_to_dto(updated)))
}
