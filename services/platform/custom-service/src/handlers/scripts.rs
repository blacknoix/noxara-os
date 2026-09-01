//! `/api/v1/custom/scripts/{slug}` — list/upsert lifecycle scripts (builder.manage).

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde::Serialize;
use utoipa::ToSchema;

use super::{internal, require_perm, set_org, validation};
use crate::auth::AuthCtx;
use crate::sandbox::parse_program;
use crate::state::AppState;
use crate::types::{CustomScriptDto, UpsertScriptRequest};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/custom/scripts/{slug}",
        get(list_scripts).put(upsert_script),
    )
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ScriptListResponse {
    pub items: Vec<CustomScriptDto>,
}

const ALLOWED_HOOKS: &[&str] = &["before_save", "after_save"];

#[utoipa::path(
    get,
    path = "/api/v1/custom/scripts/{slug}",
    tag = "custom-scripts",
    responses((status = 200, body = ScriptListResponse))
)]
pub async fn list_scripts(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(slug): Path<String>,
) -> Result<Json<ScriptListResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_manage()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        r#"
        SELECT public_id, entity_slug, hook, source, enabled
        FROM custom_script
        WHERE org_id = $1 AND entity_slug = $2
        ORDER BY hook
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&slug)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(ScriptListResponse {
        items: rows
            .into_iter()
            .map(|(id, entity_slug, hook, source, enabled)| CustomScriptDto {
                id,
                entity_slug,
                hook,
                source,
                enabled,
            })
            .collect(),
    }))
}

#[utoipa::path(
    put,
    path = "/api/v1/custom/scripts/{slug}",
    tag = "custom-scripts",
    request_body = UpsertScriptRequest,
    responses((status = 200, body = CustomScriptDto))
)]
pub async fn upsert_script(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(slug): Path<String>,
    Json(body): Json<UpsertScriptRequest>,
) -> Result<Json<CustomScriptDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_manage()).await?;

    if !ALLOWED_HOOKS.contains(&body.hook.as_str()) {
        return Err(validation(
            rid,
            format!("hook must be one of: {}", ALLOWED_HOOKS.join(", ")),
        ));
    }
    // Fail closed on invalid program at write time.
    parse_program(&body.source).map_err(|e| validation(rid, format!("invalid script: {e}")))?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let existing: Option<(uuid::Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, public_id FROM custom_script
        WHERE org_id = $1 AND entity_slug = $2 AND hook = $3
        FOR UPDATE
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&slug)
    .bind(&body.hook)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let public_id = if let Some((id, public_id)) = existing {
        sqlx::query(
            r#"
            UPDATE custom_script
            SET source = $4, enabled = $5, updated_at = now()
            WHERE org_id = $1 AND id = $2 AND hook = $3
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(id)
        .bind(&body.hook)
        .bind(&body.source)
        .bind(body.enabled)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;
        public_id
    } else {
        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::CustomScript, id);
        sqlx::query(
            r#"
            INSERT INTO custom_script (
                id, org_id, public_id, entity_slug, hook, source, enabled, created_by
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
            "#,
        )
        .bind(id)
        .bind(auth.ctx.org_id.as_uuid())
        .bind(public_id.as_str())
        .bind(&slug)
        .bind(&body.hook)
        .bind(&body.source)
        .bind(body.enabled)
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
        "custom.script.upsert",
        "custom_script",
        &public_id,
        serde_json::json!({ "entity_slug": slug, "hook": body.hook }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(CustomScriptDto {
        id: public_id,
        entity_slug: slug,
        hook: body.hook,
        source: body.source,
        enabled: body.enabled,
    }))
}
