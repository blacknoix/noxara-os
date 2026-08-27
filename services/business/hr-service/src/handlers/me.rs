//! `/api/v1/people/me` — self-service non-restricted profile.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;

use super::employees::{fetch_employee_by_user, EmployeeRow};
use super::{internal, not_found, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::principal::load_membership_scope;
use crate::state::AppState;
use crate::types::{EmployeeDto, UpdateSelfProfileRequest};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/people/me", get(get_me).patch(patch_me))
}

/// GET /api/v1/people/me
#[utoipa::path(
    get,
    path = "/api/v1/people/me",
    tag = "people-me",
    responses((status = 200, body = EmployeeDto), (status = 404))
)]
pub async fn get_me(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<EmployeeDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    // Ensure active membership; no elevated HR permission required for self.
    let _membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_employee_by_user(&mut tx, org_id, actor)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee profile"))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_directory_dto()))
}

/// PATCH /api/v1/people/me
#[utoipa::path(
    patch,
    path = "/api/v1/people/me",
    tag = "people-me",
    request_body = UpdateSelfProfileRequest,
    responses((status = 200, body = EmployeeDto), (status = 404))
)]
pub async fn patch_me(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<UpdateSelfProfileRequest>,
) -> Result<Json<EmployeeDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let _membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_employee_by_user(&mut tx, org_id, actor)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee profile"))?;

    let display_name = body
        .display_name
        .unwrap_or_else(|| row.display_name.clone());
    if display_name.trim().is_empty() {
        return Err(validation(&request_id, "display_name must not be empty"));
    }

    let updated: EmployeeRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_employee SET
            display_name = $3,
            personal_email = $4,
            phone = $5,
            location = $6,
            version = version + 1,
            updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        RETURNING {}
        "#,
        super::employees::EMPLOYEE_COLUMNS
    ))
    .bind(org_id)
    .bind(row.id)
    .bind(display_name.trim())
    .bind(body.personal_email.or(row.personal_email))
    .bind(body.phone.or(row.phone))
    .bind(body.location.or(row.location))
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.employee.self_update",
        "employee",
        &updated.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_directory_dto()))
}
