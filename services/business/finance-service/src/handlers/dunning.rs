//! `/api/v1/finance/dunning/...` — configurable dunning profiles.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use super::{conflict, if_match_version, internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreateDunningProfileRequest, DunningProfileDto, DunningProfileListResponse,
    DunningScheduleQuery, DunningScheduleResponse, DunningStepDto,
    SetCustomerDunningProfileRequest, UpdateDunningProfileRequest,
};

/// Classic ladder matching the pre-Phase-3.5 hardcoded InvoiceDunning offsets.
fn classic_default_steps() -> Vec<DunningStepDto> {
    vec![
        DunningStepDto {
            offset_days: -3,
            channel: "email".into(),
            label: "pre_due".into(),
        },
        DunningStepDto {
            offset_days: 3,
            channel: "email".into(),
            label: "reminder_1".into(),
        },
        DunningStepDto {
            offset_days: 7,
            channel: "email".into(),
            label: "reminder_2".into(),
        },
        DunningStepDto {
            offset_days: 14,
            channel: "email".into(),
            label: "final_notice".into(),
        },
    ]
}

/// Map profile steps → offset days for InvoiceDunning::start().
pub fn profile_steps_to_offsets(steps: &[DunningStepDto]) -> Vec<i32> {
    steps.iter().map(|s| s.offset_days).collect()
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/dunning/profiles",
            get(list_profiles).post(create_profile),
        )
        .route(
            "/api/v1/finance/dunning/profiles/{id}",
            get(get_profile).patch(update_profile),
        )
        .route(
            "/api/v1/finance/customers/{id}/dunning-profile",
            post(set_customer_dunning_profile),
        )
        .route(
            "/api/v1/finance/dunning/schedule",
            get(get_dunning_schedule),
        )
}

#[derive(Debug, sqlx::FromRow)]
struct ProfileRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    name: String,
    is_default: bool,
    steps: JsonValue,
    version: i32,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
}

fn steps_from_json(value: &JsonValue) -> Vec<DunningStepDto> {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

fn row_to_dto(row: ProfileRow) -> DunningProfileDto {
    DunningProfileDto {
        id: row.public_id,
        name: row.name,
        is_default: row.is_default,
        steps: steps_from_json(&row.steps),
        version: row.version,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    }
}

fn validate_steps(request_id: &str, steps: &[DunningStepDto]) -> Result<(), AppError> {
    if steps.is_empty() {
        return Err(validation(request_id, "steps must be non-empty"));
    }
    for s in steps {
        if s.channel.trim().is_empty() || s.label.trim().is_empty() {
            return Err(validation(
                request_id,
                "each step requires channel and label",
            ));
        }
    }
    Ok(())
}

/// Ensure a default dunning profile exists for the org (offsets [-3,3,7,14]).
pub async fn ensure_default_dunning_profile(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<(Uuid, String, Vec<DunningStepDto>), sqlx::Error> {
    let existing: Option<(Uuid, String, JsonValue)> = sqlx::query_as(
        r#"
        SELECT id, public_id, steps FROM finance_dunning_profile
        WHERE org_id = $1 AND is_default = true
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((id, public_id, steps)) = existing {
        return Ok((id, public_id, steps_from_json(&steps)));
    }

    let public_id = PublicId::generate(IdKind::DunningProfile);
    let id = public_id.uuid();
    let steps = classic_default_steps();
    let steps_json = serde_json::to_value(&steps).unwrap_or_else(|_| serde_json::json!([]));
    sqlx::query(
        r#"
        INSERT INTO finance_dunning_profile (
            id, org_id, public_id, name, is_default, steps
        ) VALUES ($1,$2,$3,'Default',true,$4)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&steps_json)
    .execute(&mut **tx)
    .await?;
    Ok((id, public_id.as_str().to_string(), steps))
}

/// Resolve dunning schedule for a customer (override → org default).
pub async fn resolve_dunning_schedule(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    customer_id: Uuid,
) -> Result<(Uuid, String, Vec<DunningStepDto>), sqlx::Error> {
    let override_row: Option<(Uuid, String, JsonValue)> = sqlx::query_as(
        r#"
        SELECT p.id, p.public_id, p.steps
        FROM finance_customer c
        JOIN finance_dunning_profile p ON p.id = c.dunning_profile_id
        WHERE c.org_id = $1 AND c.id = $2
        "#,
    )
    .bind(org_id)
    .bind(customer_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((id, public_id, steps)) = override_row {
        return Ok((id, public_id, steps_from_json(&steps)));
    }
    ensure_default_dunning_profile(tx, org_id).await
}

/// GET /api/v1/finance/dunning/profiles
#[utoipa::path(get, path = "/api/v1/finance/dunning/profiles", tag = "finance-dunning",
    responses((status = 200, body = DunningProfileListResponse)))]
pub async fn list_profiles(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<DunningProfileListResponse>, AppError> {
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
        perms::finance_dunning_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_default_dunning_profile(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let rows: Vec<ProfileRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, name, is_default, steps, version, created_at, updated_at
        FROM finance_dunning_profile WHERE org_id = $1 ORDER BY name ASC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let total = rows.len() as i64;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(DunningProfileListResponse {
        items: rows.into_iter().map(row_to_dto).collect(),
        total,
    }))
}

/// POST /api/v1/finance/dunning/profiles
#[utoipa::path(post, path = "/api/v1/finance/dunning/profiles", tag = "finance-dunning",
    request_body = CreateDunningProfileRequest,
    responses((status = 201, body = DunningProfileDto)))]
pub async fn create_profile(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateDunningProfileRequest>,
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
        perms::finance_dunning_manage(),
        &request_id,
    )?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name is required"));
    }
    validate_steps(&request_id, &body.steps)?;

    let public_id = PublicId::generate(IdKind::DunningProfile);
    let id = public_id.uuid();
    let steps_json = serde_json::to_value(&body.steps).unwrap_or_default();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if body.is_default {
        sqlx::query("UPDATE finance_dunning_profile SET is_default = false WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    }

    sqlx::query(
        r#"
        INSERT INTO finance_dunning_profile (
            id, org_id, public_id, name, is_default, steps
        ) VALUES ($1,$2,$3,$4,$5,$6)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(body.is_default)
    .bind(&steps_json)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.dunning_profile.create",
        "dunning_profile",
        &public_id.as_str(),
        serde_json::json!({ "name": body.name.trim() }),
    )
    .await
    .map_err(internal(&request_id))?;

    let row: ProfileRow = sqlx::query_as(
        r#"
        SELECT id, public_id, name, is_default, steps, version, created_at, updated_at
        FROM finance_dunning_profile WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(row_to_dto(row))))
}

/// GET /api/v1/finance/dunning/profiles/{id}
#[utoipa::path(get, path = "/api/v1/finance/dunning/profiles/{id}", tag = "finance-dunning",
    responses((status = 200, body = DunningProfileDto)))]
pub async fn get_profile(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<DunningProfileDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let profile_id = parse_public_id(IdKind::DunningProfile, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_dunning_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<ProfileRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, name, is_default, steps, version, created_at, updated_at
        FROM finance_dunning_profile WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(profile_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    let row = row.ok_or_else(|| not_found(&request_id, "dunning profile"))?;
    Ok(Json(row_to_dto(row)))
}

/// PATCH /api/v1/finance/dunning/profiles/{id}
#[utoipa::path(patch, path = "/api/v1/finance/dunning/profiles/{id}", tag = "finance-dunning",
    request_body = UpdateDunningProfileRequest,
    responses((status = 200, body = DunningProfileDto)))]
pub async fn update_profile(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateDunningProfileRequest>,
) -> Result<Json<DunningProfileDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let profile_id = parse_public_id(IdKind::DunningProfile, &id, &request_id)?;
    let expected = if_match_version(&headers);
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_dunning_manage(),
        &request_id,
    )?;

    if let Some(ref steps) = body.steps {
        validate_steps(&request_id, steps)?;
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<ProfileRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, name, is_default, steps, version, created_at, updated_at
        FROM finance_dunning_profile WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(profile_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let row = row.ok_or_else(|| not_found(&request_id, "dunning profile"))?;
    if let Some(exp) = expected {
        if row.version != exp {
            return Err(conflict(
                &request_id,
                format!("version mismatch: expected {exp}, got {}", row.version),
            ));
        }
    }

    if body.is_default == Some(true) {
        sqlx::query(
            "UPDATE finance_dunning_profile SET is_default = false WHERE org_id = $1 AND id <> $2",
        )
        .bind(org_id)
        .bind(profile_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let name = body.name.as_deref().unwrap_or(&row.name);
    let steps_json = body
        .steps
        .as_ref()
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .unwrap_or(row.steps.clone());
    let is_default = body.is_default.unwrap_or(row.is_default);

    sqlx::query(
        r#"
        UPDATE finance_dunning_profile SET
            name = $3, steps = $4, is_default = $5,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(profile_id)
    .bind(name)
    .bind(&steps_json)
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
        "finance.dunning_profile.update",
        "dunning_profile",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let updated: ProfileRow = sqlx::query_as(
        r#"
        SELECT id, public_id, name, is_default, steps, version, created_at, updated_at
        FROM finance_dunning_profile WHERE id = $1
        "#,
    )
    .bind(profile_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row_to_dto(updated)))
}

/// POST /api/v1/finance/customers/{id}/dunning-profile
#[utoipa::path(post, path = "/api/v1/finance/customers/{id}/dunning-profile",
    tag = "finance-dunning",
    request_body = SetCustomerDunningProfileRequest,
    responses((status = 200)))]
pub async fn set_customer_dunning_profile(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<SetCustomerDunningProfileRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let _ = parse_public_id(IdKind::Customer, &id, &request_id)?;
    let profile_uuid = parse_public_id(IdKind::DunningProfile, &body.profile_id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_dunning_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let customer: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM finance_customer WHERE org_id = $1 AND public_id = $2")
            .bind(org_id)
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    let customer_id = customer
        .map(|c| c.0)
        .ok_or_else(|| not_found(&request_id, "customer"))?;

    let profile_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM finance_dunning_profile WHERE org_id = $1 AND id = $2)",
    )
    .bind(org_id)
    .bind(profile_uuid)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    if !profile_ok {
        return Err(not_found(&request_id, "dunning profile"));
    }

    sqlx::query(
        r#"
        UPDATE finance_customer
        SET dunning_profile_id = $3, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(customer_id)
    .bind(profile_uuid)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(serde_json::json!({
        "customer_id": id,
        "profile_id": body.profile_id,
    })))
}

/// GET /api/v1/finance/dunning/schedule?invoice_id= or customer_id=
#[utoipa::path(get, path = "/api/v1/finance/dunning/schedule", tag = "finance-dunning",
    params(
        ("invoice_id" = Option<String>, Query),
        ("customer_id" = Option<String>, Query),
    ),
    responses((status = 200, body = DunningScheduleResponse)))]
pub async fn get_dunning_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<DunningScheduleQuery>,
) -> Result<Json<DunningScheduleResponse>, AppError> {
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
        perms::finance_dunning_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let customer_id = if let Some(ref inv) = q.invoice_id {
        let invoice_uuid = parse_public_id(IdKind::Invoice, inv, &request_id)?;
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT customer_id FROM finance_invoice WHERE org_id = $1 AND id = $2")
                .bind(org_id)
                .bind(invoice_uuid)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal(&request_id))?;
        row.map(|r| r.0)
            .ok_or_else(|| not_found(&request_id, "invoice"))?
    } else if let Some(ref cus) = q.customer_id {
        let _: Uuid = parse_public_id(IdKind::Customer, cus, &request_id)?;
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM finance_customer WHERE org_id = $1 AND public_id = $2")
                .bind(org_id)
                .bind(cus)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal(&request_id))?;
        row.map(|r| r.0)
            .ok_or_else(|| not_found(&request_id, "customer"))?
    } else {
        return Err(validation(
            &request_id,
            "invoice_id or customer_id is required",
        ));
    };

    let (_id, public_id, steps) = resolve_dunning_schedule(&mut tx, org_id, customer_id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    let schedule_offsets_days = profile_steps_to_offsets(&steps);
    Ok(Json(DunningScheduleResponse {
        profile_id: public_id,
        schedule_offsets_days,
        steps,
    }))
}
