//! GET/PATCH /api/v1/ai/settings

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_tenancy::set_session_org_id;
use serde_json::json;

use crate::auth::AuthCtx;
use crate::handlers::common::{
    enforce_perm, ensure_settings_row, load_settings, resolve_principal,
};
use crate::state::AppState;
use crate::types::{AiSettings, UpdateAiSettingsRequest};
use companyos_errors::{AppError, ErrorCode};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/ai/settings",
        get(get_settings).patch(patch_settings),
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/settings",
    responses((status = 200, body = AiSettings)),
    tag = "ai"
)]
pub async fn get_settings(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<AiSettings>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_settings_read(), &request_id)?;

    ensure_settings_row(&state, org_id, &request_id).await?;
    let settings = load_settings(&state, org_id, &request_id).await?;
    Ok(Json(settings))
}

#[utoipa::path(
    patch,
    path = "/api/v1/ai/settings",
    request_body = UpdateAiSettingsRequest,
    responses((status = 200, body = AiSettings)),
    tag = "ai"
)]
pub async fn patch_settings(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> Result<Json<AiSettings>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_settings_manage(), &request_id)?;

    ensure_settings_row(&state, org_id, &request_id).await?;
    let current = load_settings(&state, org_id, &request_id).await?;

    let modules = req.modules_enabled.unwrap_or(current.modules_enabled);
    let model = req.model_preference.unwrap_or(current.model_preference);
    let allow_list = req
        .auto_execute_allow_list
        .unwrap_or(current.auto_execute_allow_list);
    let sharing = req.data_sharing.unwrap_or(current.data_sharing);
    let budget = req
        .monthly_token_budget
        .unwrap_or(current.monthly_token_budget);

    let modules_json = serde_json::to_value(&modules).unwrap_or(json!({}));
    let allow_json = serde_json::to_value(&allow_list).unwrap_or(json!([]));
    let sharing_json = serde_json::to_value(&sharing).unwrap_or(json!({}));

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    sqlx::query(
        r#"
        UPDATE ai_org_settings
        SET modules_enabled = $2,
            model_preference = $3,
            auto_execute_allow_list = $4,
            data_sharing = $5,
            monthly_token_budget = $6,
            updated_at = now()
        WHERE org_id = $1
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(modules_json)
    .bind(&model)
    .bind(allow_json)
    .bind(sharing_json)
    .bind(budget)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let settings = load_settings(&state, org_id, &request_id).await?;
    Ok(Json(settings))
}
