//! `/api/v1/custom/views/{slug}` — list/create saved views.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use super::{internal, require_perm, set_org, validation};
use crate::auth::AuthCtx;
use crate::state::AppState;
use crate::types::{CustomViewDto, UpsertViewRequest};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/custom/views/{slug}",
        get(list_views).post(create_view),
    )
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ViewListResponse {
    pub items: Vec<CustomViewDto>,
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/views/{slug}",
    tag = "custom-views",
    responses((status = 200, body = ViewListResponse))
)]
pub async fn list_views(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(slug): Path<String>,
) -> Result<Json<ViewListResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_read()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let rows: Vec<(String, String, String, Value, Value, Value)> = sqlx::query_as(
        r#"
        SELECT public_id, entity_slug, name, columns, filters, sort
        FROM custom_view
        WHERE org_id = $1 AND entity_slug = $2
        ORDER BY name
        LIMIT 200
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&slug)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(ViewListResponse {
        items: rows
            .into_iter()
            .map(
                |(id, entity_slug, name, columns, filters, sort)| CustomViewDto {
                    id,
                    entity_slug,
                    name,
                    columns,
                    filters,
                    sort,
                },
            )
            .collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/custom/views/{slug}",
    tag = "custom-views",
    request_body = UpsertViewRequest,
    responses((status = 201, body = CustomViewDto))
)]
pub async fn create_view(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(slug): Path<String>,
    Json(body): Json<UpsertViewRequest>,
) -> Result<(StatusCode, Json<CustomViewDto>), AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_manage()).await?;

    if body.name.trim().is_empty() {
        return Err(validation(rid, "name is required"));
    }

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::CustomView, id);
    let columns = if body.columns.is_null() {
        Value::Array(vec![])
    } else {
        body.columns
    };
    let filters = if body.filters.is_null() {
        Value::Array(vec![])
    } else {
        body.filters
    };
    let sort = if body.sort.is_null() {
        Value::Array(vec![])
    } else {
        body.sort
    };

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    sqlx::query(
        r#"
        INSERT INTO custom_view (
            id, org_id, public_id, entity_slug, name, columns, filters, sort, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(&slug)
    .bind(body.name.trim())
    .bind(&columns)
    .bind(&filters)
    .bind(&sort)
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
        "custom.view.create",
        "custom_view",
        &public_id.as_str(),
        serde_json::json!({ "entity_slug": slug, "name": body.name }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;

    Ok((
        StatusCode::CREATED,
        Json(CustomViewDto {
            id: public_id.as_str(),
            entity_slug: slug,
            name: body.name,
            columns,
            filters,
            sort,
        }),
    ))
}
