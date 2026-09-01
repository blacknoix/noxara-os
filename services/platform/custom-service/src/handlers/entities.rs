//! `/api/v1/custom/entities` — entity definition CRUD + publish.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    conflict, enforce_opt, internal, load_authz_principal, not_found, parse_public_id, require_perm,
    set_org, validation,
};
use crate::auth::AuthCtx;
use crate::permissions::{register_entity_permissions, validate_slug};
use crate::state::AppState;
use crate::types::{
    CreateEntityRequest, EntityDefinitionDto, FieldDef, UpdateEntityRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/custom/entities",
            get(list_entities).post(create_entity),
        )
        .route(
            "/api/v1/custom/entities/{id}",
            get(get_entity)
                .patch(update_entity)
                .delete(delete_entity),
        )
        .route(
            "/api/v1/custom/entities/{id}/publish",
            post(publish_entity),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EntityListResponse {
    pub items: Vec<EntityDefinitionDto>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteEntityRequest {
    /// Must exactly match the entity slug (type-to-confirm).
    pub confirm_slug: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

type EntityRow = (
    Uuid,
    String,
    String,
    String,
    String,
    Value,
    String,
    i32,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn map_entity(row: EntityRow, rid: &str) -> Result<EntityDefinitionDto, AppError> {
    let fields: Vec<FieldDef> = serde_json::from_value(row.5).map_err(|e| {
        AppError::new(
            companyos_errors::ErrorCode::Internal,
            rid,
            format!("entity fields json: {e}"),
        )
    })?;
    Ok(EntityDefinitionDto {
        id: row.1,
        slug: row.2,
        label: row.3,
        description: row.4,
        fields,
        status: row.6,
        published_version: row.7,
        created_at: row.8,
        updated_at: row.9,
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/entities",
    tag = "custom-entities",
    responses((status = 200, body = EntityListResponse))
)]
pub async fn list_entities(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<EntityListResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_read()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let rows: Vec<EntityRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, slug, label, description, fields, status,
               published_version, created_at, updated_at
        FROM custom_entity_definition
        WHERE org_id = $1 AND deleted_at IS NULL
        ORDER BY updated_at DESC
        LIMIT 200
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(map_entity(row, rid)?);
    }
    Ok(Json(EntityListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/custom/entities",
    tag = "custom-entities",
    request_body = CreateEntityRequest,
    responses((status = 201, body = EntityDefinitionDto))
)]
pub async fn create_entity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateEntityRequest>,
) -> Result<(StatusCode, Json<EntityDefinitionDto>), AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_manage()).await?;

    validate_slug(&body.slug).map_err(|e| validation(rid, e))?;
    if body.label.trim().is_empty() {
        return Err(validation(rid, "label is required"));
    }

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::CustomEntity, id);
    let actor = auth.ctx.actor.on_behalf_of;
    let fields_json = serde_json::to_value(&body.fields)
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, rid, e.to_string()))?;
    let now = Utc::now();

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let res = sqlx::query(
        r#"
        INSERT INTO custom_entity_definition (
            id, org_id, public_id, slug, label, description, fields,
            status, published_version, created_by, updated_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,'draft',0,$8,$8)
        "#,
    )
    .bind(id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(body.slug.trim())
    .bind(body.label.trim())
    .bind(&body.description)
    .bind(&fields_json)
    .bind(actor)
    .execute(&mut *tx)
    .await;

    if let Err(e) = res {
        if is_unique(&e) {
            return Err(conflict(rid, format!("entity slug '{}' already exists", body.slug)));
        }
        return Err(internal(rid)(e));
    }

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.entity.create",
        "custom_entity",
        &public_id.as_str(),
        serde_json::json!({ "slug": body.slug }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;

    Ok((
        StatusCode::CREATED,
        Json(EntityDefinitionDto {
            id: public_id.as_str(),
            slug: body.slug,
            label: body.label,
            description: body.description,
            fields: body.fields,
            status: "draft".into(),
            published_version: 0,
            created_at: now,
            updated_at: now,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/entities/{id}",
    tag = "custom-entities",
    responses((status = 200, body = EntityDefinitionDto), (status = 404))
)]
pub async fn get_entity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<EntityDefinitionDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_read()).await?;
    let entity_id = parse_public_id(IdKind::CustomEntity, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let row: Option<EntityRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, slug, label, description, fields, status,
               published_version, created_at, updated_at
        FROM custom_entity_definition
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    let row = row.ok_or_else(|| not_found(rid, "entity"))?;
    Ok(Json(map_entity(row, rid)?))
}

#[utoipa::path(
    patch,
    path = "/api/v1/custom/entities/{id}",
    tag = "custom-entities",
    request_body = UpdateEntityRequest,
    responses((status = 200, body = EntityDefinitionDto))
)]
pub async fn update_entity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateEntityRequest>,
) -> Result<Json<EntityDefinitionDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_manage()).await?;
    let entity_id = parse_public_id(IdKind::CustomEntity, &id, rid)?;
    let actor = auth.ctx.actor.on_behalf_of;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let row: Option<EntityRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, slug, label, description, fields, status,
               published_version, created_at, updated_at
        FROM custom_entity_definition
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let row = row.ok_or_else(|| not_found(rid, "entity"))?;

    let label = body
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(row.3.as_str());
    let description = body.description.as_deref().unwrap_or(row.4.as_str());
    let fields_json = match &body.fields {
        Some(f) => serde_json::to_value(f)
            .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, rid, e.to_string()))?,
        None => row.5.clone(),
    };

    sqlx::query(
        r#"
        UPDATE custom_entity_definition
        SET label = $3, description = $4, fields = $5, updated_by = $6, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .bind(label)
    .bind(description)
    .bind(&fields_json)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.entity.update",
        "custom_entity",
        &row.1,
        serde_json::json!({ "slug": row.2 }),
    )
    .await
    .map_err(internal(rid))?;

    let updated: EntityRow = sqlx::query_as(
        r#"
        SELECT id, public_id, slug, label, description, fields, status,
               published_version, created_at, updated_at
        FROM custom_entity_definition
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;
    Ok(Json(map_entity(updated, rid)?))
}

#[utoipa::path(
    delete,
    path = "/api/v1/custom/entities/{id}",
    tag = "custom-entities",
    request_body = DeleteEntityRequest,
    responses((status = 200, body = MessageResponse))
)]
pub async fn delete_entity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<DeleteEntityRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_manage()).await?;
    let entity_id = parse_public_id(IdKind::CustomEntity, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let row: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT public_id, slug FROM custom_entity_definition
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let (public_id, slug) = row.ok_or_else(|| not_found(rid, "entity"))?;

    if body.confirm_slug != slug {
        return Err(validation(
            rid,
            format!("confirm_slug must match entity slug '{slug}'"),
        ));
    }

    sqlx::query(
        r#"
        UPDATE custom_entity_definition
        SET deleted_at = now(), updated_by = $3, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .bind(auth.ctx.actor.on_behalf_of)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.entity.delete",
        "custom_entity",
        &public_id,
        serde_json::json!({ "slug": slug }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;
    Ok(Json(MessageResponse {
        message: "entity deleted".into(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/custom/entities/{id}/publish",
    tag = "custom-entities",
    responses((status = 200, body = EntityDefinitionDto))
)]
pub async fn publish_entity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<EntityDefinitionDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let principal = load_authz_principal(&state, &auth).await?;
    enforce_opt(&principal, perms::custom_builder_manage(), rid)?;

    let entity_id = parse_public_id(IdKind::CustomEntity, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let row: Option<(String, String, i32)> = sqlx::query_as(
        r#"
        SELECT public_id, slug, published_version
        FROM custom_entity_definition
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let (public_id, slug, prev_ver) = row.ok_or_else(|| not_found(rid, "entity"))?;

    let next_ver = prev_ver + 1;
    sqlx::query(
        r#"
        UPDATE custom_entity_definition
        SET status = 'published', published_version = $3,
            updated_by = $4, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .bind(next_ver)
    .bind(auth.ctx.actor.on_behalf_of)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    register_entity_permissions(&mut tx, auth.ctx.org_id, &slug)
        .await
        .map_err(internal(rid))?;

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.entity.publish",
        "custom_entity",
        &public_id,
        serde_json::json!({ "slug": slug, "published_version": next_ver }),
    )
    .await
    .map_err(internal(rid))?;

    let updated: EntityRow = sqlx::query_as(
        r#"
        SELECT id, public_id, slug, label, description, fields, status,
               published_version, created_at, updated_at
        FROM custom_entity_definition
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(entity_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;
    Ok(Json(map_entity(updated, rid)?))
}

fn is_unique(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) => db.code().as_deref() == Some("23505"),
        _ => false,
    }
}
