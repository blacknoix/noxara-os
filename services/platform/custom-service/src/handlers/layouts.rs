//! `/api/v1/custom/layouts/{slug}` — get/upsert form layout (one per slug).

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde_json::Value;

use super::{internal, not_found, require_perm, set_org, validation};
use crate::auth::AuthCtx;
use crate::state::AppState;
use crate::types::{CustomLayoutDto, UpsertLayoutRequest};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/custom/layouts/{slug}",
        get(get_layout).put(upsert_layout),
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/layouts/{slug}",
    tag = "custom-layouts",
    responses((status = 200, body = CustomLayoutDto), (status = 404))
)]
pub async fn get_layout(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(slug): Path<String>,
) -> Result<Json<CustomLayoutDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_read()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let row: Option<(String, String, String, Value)> = sqlx::query_as(
        r#"
        SELECT public_id, entity_slug, name, sections
        FROM custom_layout
        WHERE org_id = $1 AND entity_slug = $2
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    let (id, entity_slug, name, sections) = row.ok_or_else(|| not_found(rid, "layout"))?;
    Ok(Json(CustomLayoutDto {
        id,
        entity_slug,
        name,
        sections,
    }))
}

#[utoipa::path(
    put,
    path = "/api/v1/custom/layouts/{slug}",
    tag = "custom-layouts",
    request_body = UpsertLayoutRequest,
    responses((status = 200, body = CustomLayoutDto))
)]
pub async fn upsert_layout(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(slug): Path<String>,
    Json(body): Json<UpsertLayoutRequest>,
) -> Result<Json<CustomLayoutDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_manage()).await?;

    if body.name.trim().is_empty() {
        return Err(validation(rid, "name is required"));
    }
    let sections = if body.sections.is_null() {
        Value::Array(vec![])
    } else {
        body.sections
    };

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let existing: Option<(uuid::Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, public_id FROM custom_layout
        WHERE org_id = $1 AND entity_slug = $2
        FOR UPDATE
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let public_id = if let Some((id, public_id)) = existing {
        sqlx::query(
            r#"
            UPDATE custom_layout
            SET name = $3, sections = $4, updated_at = now()
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(id)
        .bind(body.name.trim())
        .bind(&sections)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;
        public_id
    } else {
        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::CustomLayout, id);
        sqlx::query(
            r#"
            INSERT INTO custom_layout (
                id, org_id, public_id, entity_slug, name, sections, created_by
            ) VALUES ($1,$2,$3,$4,$5,$6,$7)
            "#,
        )
        .bind(id)
        .bind(auth.ctx.org_id.as_uuid())
        .bind(public_id.as_str())
        .bind(&slug)
        .bind(body.name.trim())
        .bind(&sections)
        .bind(auth.ctx.actor.on_behalf_of)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;
        public_id.as_str()
    };

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.layout.upsert",
        "custom_layout",
        &public_id,
        serde_json::json!({ "entity_slug": slug, "name": body.name }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(CustomLayoutDto {
        id: public_id,
        entity_slug: slug,
        name: body.name,
        sections,
    }))
}
