//! `/api/v1/people/employees/{id}/assets` — HR asset assignments.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::employees::{enforce_employee_scope, fetch_employee_row};
use super::{internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{AssetDto, AssetListResponse, CreateAssetRequest};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/people/employees/{id}/assets",
        get(list_assets).post(create_asset),
    )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AssetRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    #[allow(dead_code)]
    employee_id: Uuid,
    label: String,
    asset_tag: Option<String>,
    status: String,
    assigned_at: DateTime<Utc>,
    returned_at: Option<DateTime<Utc>>,
}

impl AssetRow {
    fn into_dto(self, employee_public: &str) -> AssetDto {
        AssetDto {
            id: self.public_id,
            employee_id: employee_public.to_string(),
            label: self.label,
            asset_tag: self.asset_tag,
            status: self.status,
            assigned_at: self.assigned_at.to_rfc3339(),
            returned_at: self.returned_at.map(|t| t.to_rfc3339()),
        }
    }
}

/// GET /api/v1/people/employees/{id}/assets
#[utoipa::path(
    get,
    path = "/api/v1/people/employees/{id}/assets",
    tag = "people-assets",
    params(("id" = String, Path)),
    responses((status = 200, body = AssetListResponse), (status = 404))
)]
pub async fn list_assets(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<AssetListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_employee_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_read(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let rows: Vec<AssetRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, employee_id, label, asset_tag, status, assigned_at, returned_at
        FROM people_asset
        WHERE org_id = $1 AND employee_id = $2 AND deleted_at IS NULL
        ORDER BY assigned_at DESC
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(AssetListResponse {
        items: rows
            .into_iter()
            .map(|r| r.into_dto(&emp.public_id))
            .collect(),
    }))
}

/// POST /api/v1/people/employees/{id}/assets
#[utoipa::path(
    post,
    path = "/api/v1/people/employees/{id}/assets",
    tag = "people-assets",
    request_body = CreateAssetRequest,
    params(("id" = String, Path)),
    responses((status = 201, body = AssetDto))
)]
pub async fn create_asset(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CreateAssetRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_employee_write(),
        &request_id,
    )?;

    if body.label.trim().is_empty() {
        return Err(validation(&request_id, "label must not be empty"));
    }

    let public_id = PublicId::generate(IdKind::HrAsset);
    let id_uuid = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "asset.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_write(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let now = Utc::now();
    sqlx::query(
        r#"
        INSERT INTO people_asset (
            id, org_id, public_id, employee_id, label, asset_tag, status, assigned_at
        ) VALUES ($1,$2,$3,$4,$5,$6,'assigned',$7)
        "#,
    )
    .bind(id_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(employee_id)
    .bind(body.label.trim())
    .bind(&body.asset_tag)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.asset.create",
        "asset",
        &public_id.as_str(),
        serde_json::json!({ "employee_id": emp.public_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = AssetDto {
        id: public_id.as_str(),
        employee_id: emp.public_id,
        label: body.label.trim().to_string(),
        asset_tag: body.asset_tag,
        status: "assigned".into(),
        assigned_at: now.to_rfc3339(),
        returned_at: None,
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "asset.create",
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
