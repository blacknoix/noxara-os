//! `/api/v1/inventory/purchase-requests` — draft → submit → decide (approval
//! callback) → converted (into a purchase order).

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::{conflict, internal, normalize_paging, not_found, parse_public_id, validation};
use crate::approvals_client::{self, RequestApprovalInput};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreatePurchaseRequestRequest, DecidePurchaseRequestRequest, ListQuery, PurchaseRequestDto,
    PurchaseRequestLineDto, PurchaseRequestListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/purchase-requests",
            get(list_purchase_requests).post(create_purchase_request),
        )
        .route(
            "/api/v1/inventory/purchase-requests/{id}",
            get(get_purchase_request),
        )
        .route(
            "/api/v1/inventory/purchase-requests/{id}/submit",
            post(submit_purchase_request),
        )
        .route(
            "/api/v1/inventory/purchase-requests/{id}/decide",
            post(decide_purchase_request),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PrRow {
    id: Uuid,
    public_id: String,
    status: String,
    requester_user_id: Uuid,
    approval_id: Option<String>,
    currency: String,
    total_amount_minor: i64,
    budget_account_code: Option<String>,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const PR_COLS: &str = r#"
    id, public_id, status, requester_user_id, approval_id, currency,
    total_amount_minor, budget_account_code, notes, created_at, updated_at, version
"#;

#[derive(sqlx::FromRow)]
struct PrLineRow {
    public_id: String,
    item_public_id: String,
    qty: i64,
    unit_cost_estimate_minor: i64,
    line_amount_minor: i64,
}

async fn fetch_pr(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    pr_id: Uuid,
    request_id: &str,
) -> Result<PrRow, AppError> {
    sqlx::query_as(&format!(
        "SELECT {PR_COLS} FROM inventory_purchase_request WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(pr_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "purchase request"))
}

async fn fetch_pr_lines(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    pr_id: Uuid,
    request_id: &str,
) -> Result<Vec<PurchaseRequestLineDto>, AppError> {
    let rows: Vec<PrLineRow> = sqlx::query_as(
        r#"
        SELECT l.public_id, i.public_id AS item_public_id, l.qty,
               l.unit_cost_estimate_minor, l.line_amount_minor
        FROM inventory_purchase_request_line l
        JOIN inventory_item i ON i.id = l.item_id AND i.org_id = l.org_id
        WHERE l.org_id = $1 AND l.request_id = $2
        ORDER BY l.created_at
        "#,
    )
    .bind(org_id)
    .bind(pr_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(rows
        .into_iter()
        .map(|r| PurchaseRequestLineDto {
            id: r.public_id,
            item_id: r.item_public_id,
            qty: r.qty,
            unit_cost_estimate_minor: r.unit_cost_estimate_minor,
            line_amount_minor: r.line_amount_minor,
        })
        .collect())
}

async fn to_dto(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    row: PrRow,
    request_id: &str,
) -> Result<PurchaseRequestDto, AppError> {
    let lines = fetch_pr_lines(tx, org_id, row.id, request_id).await?;
    Ok(PurchaseRequestDto {
        id: row.public_id,
        status: row.status,
        requester_user_id: PublicId::new(IdKind::User, row.requester_user_id).as_str(),
        approval_id: row.approval_id,
        currency: row.currency,
        total_amount_minor: row.total_amount_minor,
        budget_account_code: row.budget_account_code,
        notes: row.notes,
        lines,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        version: row.version,
    })
}

/// GET /api/v1/inventory/purchase-requests
#[utoipa::path(get, path = "/api/v1/inventory/purchase-requests", tag = "inventory-purchase-requests",
    params(ListQuery), responses((status = 200, body = PurchaseRequestListResponse)))]
pub async fn list_purchase_requests(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<PurchaseRequestListResponse>, AppError> {
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
    let perm = perms::inventory_purchase_request_read();
    enforce_any_scope(&membership.principal, perm.clone(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perm);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM inventory_purchase_request WHERE org_id = ");
    count_qb.push_bind(org_id);
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
        "SELECT {PR_COLS} FROM inventory_purchase_request WHERE org_id = "
    ));
    qb.push_bind(org_id);
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
    );
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<PrRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(to_dto(&mut tx, org_id, row, &request_id).await?);
    }
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(PurchaseRequestListResponse { items, total }))
}

/// POST /api/v1/inventory/purchase-requests
#[utoipa::path(post, path = "/api/v1/inventory/purchase-requests", tag = "inventory-purchase-requests",
    request_body = CreatePurchaseRequestRequest, responses((status = 201, body = PurchaseRequestDto)))]
pub async fn create_purchase_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreatePurchaseRequestRequest>,
) -> Result<(axum::http::StatusCode, Json<PurchaseRequestDto>), AppError> {
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
        perms::inventory_purchase_request_write(),
        &request_id,
    )?;

    if body.lines.is_empty() {
        return Err(validation(&request_id, "at least one line is required"));
    }
    let _currency: companyos_money::Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let pr_id = new_uuid_v7();
    let pr_public_id = PublicId::new(IdKind::PurchaseRequest, pr_id);
    let mut total_amount_minor: i64 = 0;

    for line in &body.lines {
        if line.qty <= 0 {
            return Err(validation(&request_id, "line qty must be > 0"));
        }
        if line.unit_cost_estimate_minor < 0 {
            return Err(validation(
                &request_id,
                "unit_cost_estimate_minor must be >= 0",
            ));
        }
        let item_id = parse_public_id(IdKind::InventoryItem, &line.item_id, &request_id)?;
        let item_exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM inventory_item WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .bind(item_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if item_exists.is_none() {
            return Err(not_found(&request_id, "item"));
        }
        let line_amount = line.qty.saturating_mul(line.unit_cost_estimate_minor);
        total_amount_minor = total_amount_minor.saturating_add(line_amount);
    }

    sqlx::query(
        r#"
        INSERT INTO inventory_purchase_request (
            id, org_id, public_id, status, requester_user_id, currency,
            total_amount_minor, budget_account_code, notes, owner_user_id
        ) VALUES ($1,$2,$3,'draft',$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(pr_id)
    .bind(org_id)
    .bind(pr_public_id.as_str())
    .bind(auth.ctx.actor.user_id)
    .bind(&body.currency)
    .bind(total_amount_minor)
    .bind(body.budget_account_code.as_deref())
    .bind(body.notes.as_deref())
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for line in &body.lines {
        let item_id = parse_public_id(IdKind::InventoryItem, &line.item_id, &request_id)?;
        let line_id = new_uuid_v7();
        let line_public_id = PublicId::new(IdKind::PurchaseRequestLine, line_id);
        let line_amount = line.qty.saturating_mul(line.unit_cost_estimate_minor);
        sqlx::query(
            r#"
            INSERT INTO inventory_purchase_request_line (
                id, org_id, public_id, request_id, item_id, qty, unit_cost_estimate_minor, line_amount_minor
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
            "#,
        )
        .bind(line_id)
        .bind(org_id)
        .bind(line_public_id.as_str())
        .bind(pr_id)
        .bind(item_id)
        .bind(line.qty)
        .bind(line.unit_cost_estimate_minor)
        .bind(line_amount)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let row = fetch_pr(&mut tx, org_id, pr_id, &request_id).await?;
    let dto = to_dto(&mut tx, org_id, row, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "purchase_request",
        "drafted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "total_amount_minor": dto.total_amount_minor }),
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
        "inventory.purchase_request.create",
        "purchase_request",
        &dto.id,
        serde_json::json!({ "total_amount_minor": dto.total_amount_minor }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((axum::http::StatusCode::CREATED, Json(dto)))
}

/// GET /api/v1/inventory/purchase-requests/{id}
#[utoipa::path(get, path = "/api/v1/inventory/purchase-requests/{id}", tag = "inventory-purchase-requests",
    params(("id" = String, Path)), responses((status = 200, body = PurchaseRequestDto), (status = 404)))]
pub async fn get_purchase_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<PurchaseRequestDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let pr_id = parse_public_id(IdKind::PurchaseRequest, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_purchase_request_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_pr(&mut tx, org_id, pr_id, &request_id).await?;
    let dto = to_dto(&mut tx, org_id, row, &request_id).await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/inventory/purchase-requests/{id}/submit
#[utoipa::path(post, path = "/api/v1/inventory/purchase-requests/{id}/submit", tag = "inventory-purchase-requests",
    params(("id" = String, Path)), responses((status = 200, body = PurchaseRequestDto)))]
pub async fn submit_purchase_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<PurchaseRequestDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let pr_id = parse_public_id(IdKind::PurchaseRequest, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_purchase_request_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_pr(&mut tx, org_id, pr_id, &request_id).await?;
    if row.status != "draft" {
        return Err(conflict(
            &request_id,
            format!("purchase request status {} cannot be submitted", row.status),
        ));
    }

    let approval_id = approvals_client::request_approval(
        &auth,
        RequestApprovalInput {
            subject_type: "purchase_request",
            subject_id: &row.public_id,
            title: format!("Purchase request {}", row.public_id),
            summary: row.notes.clone(),
            amount_minor: Some(row.total_amount_minor),
            currency: Some(row.currency.clone()),
            category: row.budget_account_code.clone(),
        },
    )
    .await;

    let updated: PrRow = sqlx::query_as(&format!(
        r#"
        UPDATE inventory_purchase_request SET
            status = 'pending_approval', approval_id = $3,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {PR_COLS}
        "#
    ))
    .bind(org_id)
    .bind(pr_id)
    .bind(approval_id.as_deref())
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let dto = to_dto(&mut tx, org_id, updated, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "purchase_request",
        "submitted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "approval_id": dto.approval_id }),
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
        "inventory.purchase_request.submit",
        "purchase_request",
        &dto.id,
        serde_json::json!({ "approval_id": dto.approval_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/inventory/purchase-requests/{id}/decide — approval callback
/// from `companyos-project` (approvals). Requires the same write permission;
/// the callback authenticates as an actor with organization-scope access
/// (mirrors `hr-service`'s payroll `/decide` endpoint).
#[utoipa::path(post, path = "/api/v1/inventory/purchase-requests/{id}/decide", tag = "inventory-purchase-requests",
    request_body = DecidePurchaseRequestRequest, params(("id" = String, Path)),
    responses((status = 200, body = PurchaseRequestDto)))]
pub async fn decide_purchase_request(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<DecidePurchaseRequestRequest>,
) -> Result<Json<PurchaseRequestDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let pr_id = parse_public_id(IdKind::PurchaseRequest, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_purchase_request_write(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let row = fetch_pr(&mut tx, org_id, pr_id, &request_id).await?;
    if row.status == "approved" || row.status == "rejected" || row.status == "converted" {
        let dto = to_dto(&mut tx, org_id, row, &request_id).await?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(dto));
    }
    if row.status != "pending_approval" && row.status != "draft" {
        return Err(conflict(
            &request_id,
            format!("purchase request status {} cannot be decided", row.status),
        ));
    }

    let new_status = if body.approve { "approved" } else { "rejected" };
    let updated: PrRow = sqlx::query_as(&format!(
        r#"
        UPDATE inventory_purchase_request SET
            status = $3, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        RETURNING {PR_COLS}
        "#
    ))
    .bind(org_id)
    .bind(pr_id)
    .bind(new_status)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let dto = to_dto(&mut tx, org_id, updated, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Inventory,
        "purchase_request",
        if body.approve { "approved" } else { "rejected" },
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": dto.id, "note": body.note }),
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
        "inventory.purchase_request.decide",
        "purchase_request",
        &dto.id,
        serde_json::json!({ "approve": body.approve }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
