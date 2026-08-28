//! `/api/v1/inventory/assets` — fixed-asset register (NOT `people_asset` / HR
//! asset assignments — this is inventory-owned capital equipment tracking).
//!
//! Covers CRUD, assign/return (opaque `emp_…` employee reference, no FK to
//! People tables), straight-line depreciation (posts a Dr Depreciation
//! Expense / Cr Accumulated Depreciation journal to finance-service), and
//! maintenance schedules (`/api/v1/inventory/maintenance/due`).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::{conflict, internal, normalize_paging, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::finance_client;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    AssetAssignmentDto, AssignAssetRequest, CreateInventoryAssetRequest,
    CreateMaintenanceScheduleRequest, DepreciateAssetRequest, DepreciateAssetResponse,
    InventoryAssetDto, InventoryAssetListResponse, ListQuery, MaintenanceScheduleDto,
    MaintenanceScheduleListResponse, ReturnAssetRequest, UpdateInventoryAssetRequest,
};
use crate::valuation;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/assets",
            get(list_assets).post(create_asset),
        )
        .route(
            "/api/v1/inventory/assets/{id}",
            get(get_asset).patch(update_asset),
        )
        .route("/api/v1/inventory/assets/{id}/assign", post(assign_asset))
        .route("/api/v1/inventory/assets/{id}/return", post(return_asset))
        .route(
            "/api/v1/inventory/assets/{id}/depreciate",
            post(depreciate_asset),
        )
        .route(
            "/api/v1/inventory/assets/{id}/maintenance-schedules",
            post(create_maintenance_schedule),
        )
        .route(
            "/api/v1/inventory/maintenance/due",
            get(list_maintenance_due),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AssetRow {
    public_id: String,
    item_public_id: Option<String>,
    name: String,
    asset_tag: Option<String>,
    status: String,
    acquisition_cost_minor: i64,
    currency: String,
    acquired_at: Option<NaiveDate>,
    useful_life_months: i32,
    salvage_minor: i64,
    accumulated_depreciation_minor: i64,
    last_depreciated_at: Option<NaiveDate>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl AssetRow {
    fn into_dto(self) -> InventoryAssetDto {
        InventoryAssetDto {
            id: self.public_id,
            item_id: self.item_public_id,
            name: self.name,
            asset_tag: self.asset_tag,
            status: self.status,
            acquisition_cost_minor: self.acquisition_cost_minor,
            currency: self.currency,
            acquired_at: self.acquired_at.map(|d| d.to_string()),
            useful_life_months: self.useful_life_months,
            salvage_minor: self.salvage_minor,
            accumulated_depreciation_minor: self.accumulated_depreciation_minor,
            last_depreciated_at: self.last_depreciated_at.map(|d| d.to_string()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

const COLS: &str = r#"
    a.public_id, i.public_id AS item_public_id, a.name, a.asset_tag, a.status,
    a.acquisition_cost_minor, a.currency, a.acquired_at, a.useful_life_months,
    a.salvage_minor, a.accumulated_depreciation_minor, a.last_depreciated_at,
    a.created_at, a.updated_at, a.version
"#;

async fn fetch_asset(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    asset_id: Uuid,
    request_id: &str,
) -> Result<AssetRow, AppError> {
    sqlx::query_as(&format!(
        r#"
        SELECT {COLS}
        FROM inventory_asset a
        LEFT JOIN inventory_item i ON i.id = a.item_id AND i.org_id = a.org_id
        WHERE a.org_id = $1 AND a.id = $2 AND a.deleted_at IS NULL
        "#
    ))
    .bind(org_id)
    .bind(asset_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "asset"))
}

/// Number of whole months elapsed from `from` through `to` (0 if `to <= from`).
fn months_elapsed(from: NaiveDate, to: NaiveDate) -> i32 {
    if to <= from {
        return 0;
    }
    let mut months = (to.year() - from.year()) * 12 + (to.month() as i32 - from.month() as i32);
    if to.day() < from.day() {
        months -= 1;
    }
    months.max(0)
}

/// GET /api/v1/inventory/assets
#[utoipa::path(get, path = "/api/v1/inventory/assets", tag = "inventory-assets",
    params(ListQuery), responses((status = 200, body = InventoryAssetListResponse)))]
pub async fn list_assets(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<InventoryAssetListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    let perm = perms::inventory_asset_read();
    enforce_any_scope(&membership.principal, perm.clone(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perm);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM inventory_asset a WHERE a.org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND a.deleted_at IS NULL");
    push_owner_predicate(
        &mut count_qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        r#"
        SELECT {COLS}
        FROM inventory_asset a
        LEFT JOIN inventory_item i ON i.id = a.item_id AND i.org_id = a.org_id
        WHERE a.org_id = "#
    ));
    qb.push_bind(org_id);
    qb.push(" AND a.deleted_at IS NULL");
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    qb.push(" ORDER BY a.created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<AssetRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(InventoryAssetListResponse {
        items: rows.into_iter().map(AssetRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/inventory/assets
#[utoipa::path(post, path = "/api/v1/inventory/assets", tag = "inventory-assets",
    request_body = CreateInventoryAssetRequest, responses((status = 201, body = InventoryAssetDto)))]
pub async fn create_asset(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateInventoryAssetRequest>,
) -> Result<(StatusCode, Json<InventoryAssetDto>), AppError> {
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
        perms::inventory_asset_write(),
        &request_id,
    )?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name is required"));
    }
    let currency: Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;
    if body.acquisition_cost_minor < 0 {
        return Err(validation(
            &request_id,
            "acquisition_cost_minor must be >= 0",
        ));
    }
    let salvage_minor = body.salvage_minor.unwrap_or(0);
    if salvage_minor < 0 {
        return Err(validation(&request_id, "salvage_minor must be >= 0"));
    }
    let useful_life_months = body.useful_life_months.unwrap_or(36);
    if useful_life_months <= 0 {
        return Err(validation(&request_id, "useful_life_months must be > 0"));
    }
    let acquired_at = match &body.acquired_at {
        Some(s) => Some(
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| validation(&request_id, "acquired_at must be YYYY-MM-DD"))?,
        ),
        None => None,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let item_id = match &body.item_id {
        Some(raw) => {
            let iid = parse_public_id(IdKind::InventoryItem, raw, &request_id)?;
            let exists: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM inventory_item WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
            )
            .bind(org_id)
            .bind(iid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            if exists.is_none() {
                return Err(not_found(&request_id, "item"));
            }
            Some(iid)
        }
        None => None,
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::FixedAsset, id);

    sqlx::query(
        r#"
        INSERT INTO inventory_asset (
            id, org_id, public_id, item_id, name, asset_tag, acquisition_cost_minor,
            currency, acquired_at, useful_life_months, salvage_minor, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(item_id)
    .bind(body.name.trim())
    .bind(body.asset_tag.as_deref())
    .bind(body.acquisition_cost_minor)
    .bind(currency.as_str())
    .bind(acquired_at)
    .bind(useful_life_months)
    .bind(salvage_minor)
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_asset(&mut tx, org_id, id, &request_id).await?;
    let dto = row.into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "asset",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "name": dto.name }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.asset.create",
        "asset",
        &dto.id,
        serde_json::json!({ "name": dto.name }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/inventory/assets/{id}
#[utoipa::path(get, path = "/api/v1/inventory/assets/{id}", tag = "inventory-assets",
    params(("id" = String, Path)), responses((status = 200, body = InventoryAssetDto), (status = 404)))]
pub async fn get_asset(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<InventoryAssetDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let asset_id = parse_public_id(IdKind::FixedAsset, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_asset_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_asset(&mut tx, org_id, asset_id, &request_id).await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/inventory/assets/{id}
#[utoipa::path(patch, path = "/api/v1/inventory/assets/{id}", tag = "inventory-assets",
    params(("id" = String, Path)), request_body = UpdateInventoryAssetRequest,
    responses((status = 200, body = InventoryAssetDto), (status = 404)))]
pub async fn update_asset(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateInventoryAssetRequest>,
) -> Result<Json<InventoryAssetDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let asset_id = parse_public_id(IdKind::FixedAsset, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_asset_write(),
        &request_id,
    )?;

    if let Some(months) = body.useful_life_months {
        if months <= 0 {
            return Err(validation(&request_id, "useful_life_months must be > 0"));
        }
    }
    if let Some(salvage) = body.salvage_minor {
        if salvage < 0 {
            return Err(validation(&request_id, "salvage_minor must be >= 0"));
        }
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    // Ensures a 404 (not a no-op update) when the asset doesn't exist.
    let _ = fetch_asset(&mut tx, org_id, asset_id, &request_id).await?;

    sqlx::query(
        r#"
        UPDATE inventory_asset SET
            name = COALESCE($3, name),
            asset_tag = COALESCE($4, asset_tag),
            useful_life_months = COALESCE($5, useful_life_months),
            salvage_minor = COALESCE($6, salvage_minor),
            version = version + 1,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(asset_id)
    .bind(body.name.as_deref())
    .bind(body.asset_tag.as_deref())
    .bind(body.useful_life_months)
    .bind(body.salvage_minor)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row = fetch_asset(&mut tx, org_id, asset_id, &request_id).await?;
    let dto = row.into_dto();

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.asset.update",
        "asset",
        &dto.id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/inventory/assets/{id}/assign
#[utoipa::path(post, path = "/api/v1/inventory/assets/{id}/assign", tag = "inventory-assets",
    params(("id" = String, Path)), request_body = AssignAssetRequest,
    responses((status = 201, body = AssetAssignmentDto), (status = 409)))]
pub async fn assign_asset(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<AssignAssetRequest>,
) -> Result<(StatusCode, Json<AssetAssignmentDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let asset_id = parse_public_id(IdKind::FixedAsset, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_asset_write(),
        &request_id,
    )?;

    // Opaque People reference — validated for shape only, no FK / lookup
    // against `people_*` tables (inventory-service never reads those).
    let assignee: PublicId = body
        .assignee_employee_public_id
        .parse()
        .map_err(|_| validation(&request_id, "invalid assignee_employee_public_id"))?;
    if assignee.kind() != IdKind::Employee {
        return Err(validation(
            &request_id,
            "assignee_employee_public_id must be an emp_… id",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_asset(&mut tx, org_id, asset_id, &request_id).await?;
    if row.status == "disposed" {
        return Err(conflict(&request_id, "disposed asset cannot be assigned"));
    }
    if row.status == "assigned" {
        return Err(conflict(
            &request_id,
            "asset is already assigned; return it first",
        ));
    }

    let assignment_id = new_uuid_v7();
    let assignment_public_id = PublicId::new(IdKind::AssetAssignment, assignment_id);

    let assigned_at: (DateTime<Utc>,) = sqlx::query_as(
        r#"
        INSERT INTO inventory_asset_assignment (
            id, org_id, public_id, asset_id, assignee_employee_public_id, notes, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7)
        RETURNING assigned_at
        "#,
    )
    .bind(assignment_id)
    .bind(org_id)
    .bind(assignment_public_id.as_str())
    .bind(asset_id)
    .bind(body.assignee_employee_public_id.trim())
    .bind(body.notes.as_deref())
    .bind(auth.ctx.actor.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    sqlx::query(
        "UPDATE inventory_asset SET status = 'assigned', version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(asset_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = AssetAssignmentDto {
        id: assignment_public_id.as_str(),
        asset_id: row.public_id.clone(),
        assignee_employee_public_id: body.assignee_employee_public_id.trim().to_string(),
        assigned_at: assigned_at.0.to_rfc3339(),
        returned_at: None,
        notes: body.notes.clone(),
    };

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "asset",
        "assigned",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": row.public_id,
            "assignment_id": dto.id,
            "assignee_employee_public_id": dto.assignee_employee_public_id,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.asset.assign",
        "asset",
        &row.public_id,
        serde_json::json!({ "assignee_employee_public_id": dto.assignee_employee_public_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// POST /api/v1/inventory/assets/{id}/return
#[utoipa::path(post, path = "/api/v1/inventory/assets/{id}/return", tag = "inventory-assets",
    params(("id" = String, Path)), request_body = ReturnAssetRequest,
    responses((status = 200, body = AssetAssignmentDto), (status = 409)))]
pub async fn return_asset(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<ReturnAssetRequest>,
) -> Result<Json<AssetAssignmentDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let asset_id = parse_public_id(IdKind::FixedAsset, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_asset_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_asset(&mut tx, org_id, asset_id, &request_id).await?;
    if row.status != "assigned" {
        return Err(conflict(
            &request_id,
            format!("asset status {} is not assigned", row.status),
        ));
    }

    #[derive(sqlx::FromRow)]
    struct OpenAssignment {
        id: Uuid,
        public_id: String,
        assignee_employee_public_id: String,
        assigned_at: DateTime<Utc>,
    }
    let open: Option<OpenAssignment> = sqlx::query_as(
        r#"
        SELECT id, public_id, assignee_employee_public_id, assigned_at
        FROM inventory_asset_assignment
        WHERE org_id = $1 AND asset_id = $2 AND returned_at IS NULL
        ORDER BY assigned_at DESC LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(asset_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let Some(open) = open else {
        return Err(conflict(
            &request_id,
            "no open assignment found for this asset",
        ));
    };

    let returned_at: (DateTime<Utc>,) = sqlx::query_as(
        r#"
        UPDATE inventory_asset_assignment SET
            returned_at = now(),
            notes = COALESCE($3, notes)
        WHERE org_id = $1 AND id = $2
        RETURNING returned_at
        "#,
    )
    .bind(org_id)
    .bind(open.id)
    .bind(body.notes.as_deref())
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    sqlx::query(
        "UPDATE inventory_asset SET status = 'in_stock', version = version + 1, updated_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(asset_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = AssetAssignmentDto {
        id: open.public_id,
        asset_id: row.public_id.clone(),
        assignee_employee_public_id: open.assignee_employee_public_id,
        assigned_at: open.assigned_at.to_rfc3339(),
        returned_at: Some(returned_at.0.to_rfc3339()),
        notes: body.notes.clone(),
    };

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "asset",
        "returned",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": row.public_id, "assignment_id": dto.id }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.asset.return",
        "asset",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/inventory/assets/{id}/depreciate
///
/// Straight-line depreciation for whole months elapsed since
/// `last_depreciated_at` (or `acquired_at` on the first run) through
/// `as_of_date` (defaults to today). A run with zero elapsed months (e.g. a
/// same-day retry) is a no-op — no journal is posted and `last_depreciated_at`
/// is left untouched, which is what makes calling this endpoint repeatedly
/// safe without a separate Idempotency-Key ledger. Posting the journal and
/// updating the asset happen in one transaction: a finance rejection (e.g. a
/// closed fiscal period) rolls back the accumulated-depreciation update too.
#[utoipa::path(post, path = "/api/v1/inventory/assets/{id}/depreciate", tag = "inventory-assets",
    params(("id" = String, Path)), request_body = DepreciateAssetRequest,
    responses((status = 200, body = DepreciateAssetResponse)))]
pub async fn depreciate_asset(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<DepreciateAssetRequest>,
) -> Result<Json<DepreciateAssetResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let asset_id = parse_public_id(IdKind::FixedAsset, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_asset_write(),
        &request_id,
    )?;

    let as_of: NaiveDate = match &body.as_of_date {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| validation(&request_id, "as_of_date must be YYYY-MM-DD"))?,
        None => Utc::now().date_naive(),
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_asset(&mut tx, org_id, asset_id, &request_id).await?;
    let Some(acquired_at) = row.acquired_at else {
        return Err(validation(
            &request_id,
            "asset has no acquired_at date; cannot depreciate",
        ));
    };

    let from = row.last_depreciated_at.unwrap_or(acquired_at);
    let months = months_elapsed(from, as_of);
    if months <= 0 {
        let dto = row.into_dto();
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(DepreciateAssetResponse {
            asset: dto,
            depreciation_expense_minor: 0,
            journal_public_id: None,
        }));
    }

    let dep = valuation::straight_line_depreciation_minor(
        row.acquisition_cost_minor,
        row.salvage_minor,
        row.useful_life_months,
        row.accumulated_depreciation_minor,
        months,
    )
    .map_err(|e| validation(&request_id, e.to_string()))?;

    if dep <= 0 {
        let dto = row.into_dto();
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(DepreciateAssetResponse {
            asset: dto,
            depreciation_expense_minor: 0,
            journal_public_id: None,
        }));
    }

    let new_accum = row.accumulated_depreciation_minor.saturating_add(dep);

    sqlx::query(
        r#"
        UPDATE inventory_asset SET
            accumulated_depreciation_minor = $3,
            last_depreciated_at = $4,
            version = version + 1,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(asset_id)
    .bind(new_accum)
    .bind(as_of)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let journal_public_id = finance_client::post_depreciation_journal(
        &auth,
        asset_id,
        &row.currency,
        dep,
        format!("Depreciation for asset {}", row.public_id),
        None,
        &request_id,
    )
    .await?;

    let updated = fetch_asset(&mut tx, org_id, asset_id, &request_id).await?;
    let dto = updated.into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "asset",
        "depreciated",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "depreciation_expense_minor": dep,
            "accumulated_depreciation_minor": new_accum,
            "journal_public_id": journal_public_id,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.asset.depreciate",
        "asset",
        &dto.id,
        serde_json::json!({ "depreciation_expense_minor": dep, "journal_public_id": journal_public_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(DepreciateAssetResponse {
        asset: dto,
        depreciation_expense_minor: dep,
        journal_public_id: Some(journal_public_id),
    }))
}

#[derive(sqlx::FromRow)]
struct MaintenanceRow {
    public_id: String,
    asset_public_id: String,
    title: String,
    interval_days: i32,
    next_due_at: NaiveDate,
    last_completed_at: Option<NaiveDate>,
    notes: Option<String>,
}

impl MaintenanceRow {
    fn into_dto(self) -> MaintenanceScheduleDto {
        MaintenanceScheduleDto {
            id: self.public_id,
            asset_id: self.asset_public_id,
            title: self.title,
            interval_days: self.interval_days,
            next_due_at: self.next_due_at.to_string(),
            last_completed_at: self.last_completed_at.map(|d| d.to_string()),
            notes: self.notes,
        }
    }
}

/// POST /api/v1/inventory/assets/{id}/maintenance-schedules
#[utoipa::path(post, path = "/api/v1/inventory/assets/{id}/maintenance-schedules", tag = "inventory-assets",
    params(("id" = String, Path)), request_body = CreateMaintenanceScheduleRequest,
    responses((status = 201, body = MaintenanceScheduleDto)))]
pub async fn create_maintenance_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<CreateMaintenanceScheduleRequest>,
) -> Result<(StatusCode, Json<MaintenanceScheduleDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let asset_id = parse_public_id(IdKind::FixedAsset, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_asset_write(),
        &request_id,
    )?;

    if body.title.trim().is_empty() {
        return Err(validation(&request_id, "title is required"));
    }
    if body.interval_days <= 0 {
        return Err(validation(&request_id, "interval_days must be > 0"));
    }
    let next_due_at = NaiveDate::parse_from_str(&body.next_due_at, "%Y-%m-%d")
        .map_err(|_| validation(&request_id, "next_due_at must be YYYY-MM-DD"))?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let asset = fetch_asset(&mut tx, org_id, asset_id, &request_id).await?;

    let ms_id = new_uuid_v7();
    let ms_public_id = PublicId::new(IdKind::MaintenanceSchedule, ms_id);
    sqlx::query(
        r#"
        INSERT INTO inventory_maintenance_schedule (
            id, org_id, public_id, asset_id, title, interval_days, next_due_at, notes
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        "#,
    )
    .bind(ms_id)
    .bind(org_id)
    .bind(ms_public_id.as_str())
    .bind(asset_id)
    .bind(body.title.trim())
    .bind(body.interval_days)
    .bind(next_due_at)
    .bind(body.notes.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = MaintenanceScheduleDto {
        id: ms_public_id.as_str(),
        asset_id: asset.public_id.clone(),
        title: body.title.trim().to_string(),
        interval_days: body.interval_days,
        next_due_at: next_due_at.to_string(),
        last_completed_at: None,
        notes: body.notes.clone(),
    };

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.maintenance_schedule.create",
        "maintenance_schedule",
        &dto.id,
        serde_json::json!({ "asset_id": asset.public_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/inventory/maintenance/due
#[utoipa::path(get, path = "/api/v1/inventory/maintenance/due", tag = "inventory-assets",
    responses((status = 200, body = MaintenanceScheduleListResponse)))]
pub async fn list_maintenance_due(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<MaintenanceScheduleListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    let perm = perms::inventory_asset_read();
    enforce_any_scope(&membership.principal, perm.clone(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perm);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        r#"
        SELECT ms.public_id, a.public_id AS asset_public_id, ms.title, ms.interval_days,
               ms.next_due_at, ms.last_completed_at, ms.notes
        FROM inventory_maintenance_schedule ms
        JOIN inventory_asset a ON a.id = ms.asset_id AND a.org_id = ms.org_id
        WHERE ms.org_id = "#,
    );
    qb.push_bind(org_id);
    qb.push(" AND ms.next_due_at <= CURRENT_DATE AND a.deleted_at IS NULL");
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    qb.push(" ORDER BY ms.next_due_at ASC");

    let rows: Vec<MaintenanceRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(MaintenanceScheduleListResponse {
        items: rows.into_iter().map(MaintenanceRow::into_dto).collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_elapsed_basic() {
        let from = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 4, 15).unwrap();
        assert_eq!(months_elapsed(from, to), 3);
    }

    #[test]
    fn months_elapsed_partial_month_not_counted() {
        let from = NaiveDate::from_ymd_opt(2026, 1, 20).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();
        assert_eq!(months_elapsed(from, to), 0);
    }

    #[test]
    fn months_elapsed_same_or_earlier_is_zero() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(months_elapsed(d, d), 0);
        assert_eq!(months_elapsed(d, d.pred_opt().unwrap()), 0);
    }
}
