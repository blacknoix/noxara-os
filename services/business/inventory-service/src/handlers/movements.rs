//! `/api/v1/inventory/movements` — append-only stock movement ledger, plus
//! `/api/v1/inventory/stock/reconcile`.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{internal, parse_optional_public_id, validation};
use crate::auth::AuthCtx;
use crate::finance_client;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::stock::{self, PostMovementInput};
use crate::types::{
    CreateStockMovementRequest, DriftAlertDto, MovementListQuery, ReconcileStockRequest,
    ReconcileStockResponse, StockMovementDto, StockMovementListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/movements",
            get(list_movements).post(create_movement),
        )
        .route("/api/v1/inventory/stock/reconcile", post(reconcile_stock))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct MovementRow {
    public_id: String,
    warehouse_public_id: String,
    item_public_id: String,
    qty_delta: i64,
    unit_cost_minor: i64,
    currency: String,
    movement_type: String,
    source_type: Option<String>,
    source_id: Option<Uuid>,
    memo: Option<String>,
    created_at: DateTime<Utc>,
}

impl MovementRow {
    fn into_dto(self) -> StockMovementDto {
        StockMovementDto {
            id: self.public_id,
            warehouse_id: self.warehouse_public_id,
            item_id: self.item_public_id,
            qty_delta: self.qty_delta,
            unit_cost_minor: self.unit_cost_minor,
            currency: self.currency,
            movement_type: self.movement_type,
            source_type: self.source_type,
            source_id: self.source_id.map(|u| u.to_string()),
            memo: self.memo,
            created_at: self.created_at.to_rfc3339(),
            cogs_journal_public_id: None,
            qty_on_hand_after: 0,
            avg_unit_cost_minor_after: 0,
            low_stock: false,
        }
    }
}

const COLS: &str = r#"
    m.public_id, w.public_id AS warehouse_public_id, i.public_id AS item_public_id,
    m.qty_delta, m.unit_cost_minor, m.currency, m.movement_type, m.source_type,
    m.source_id, m.memo, m.created_at
"#;

/// GET /api/v1/inventory/movements
#[utoipa::path(get, path = "/api/v1/inventory/movements", tag = "inventory-movements",
    params(MovementListQuery), responses((status = 200, body = StockMovementListResponse)))]
pub async fn list_movements(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<MovementListQuery>,
) -> Result<Json<StockMovementListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let (limit, offset) = super::normalize_paging(q.limit, q.offset);
    let warehouse_id =
        parse_optional_public_id(IdKind::Warehouse, q.warehouse_id.as_deref(), &request_id)?;
    let item_id =
        parse_optional_public_id(IdKind::InventoryItem, q.item_id.as_deref(), &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_stock_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM inventory_stock_movement m WHERE m.org_id = ");
    count_qb.push_bind(org_id);
    if let Some(w) = warehouse_id {
        count_qb.push(" AND m.warehouse_id = ");
        count_qb.push_bind(w);
    }
    if let Some(i) = item_id {
        count_qb.push(" AND m.item_id = ");
        count_qb.push_bind(i);
    }
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        r#"
        SELECT {COLS}
        FROM inventory_stock_movement m
        JOIN inventory_warehouse w ON w.id = m.warehouse_id AND w.org_id = m.org_id
        JOIN inventory_item i ON i.id = m.item_id AND i.org_id = m.org_id
        WHERE m.org_id = "#
    ));
    qb.push_bind(org_id);
    if let Some(w) = warehouse_id {
        qb.push(" AND m.warehouse_id = ");
        qb.push_bind(w);
    }
    if let Some(i) = item_id {
        qb.push(" AND m.item_id = ");
        qb.push_bind(i);
    }
    qb.push(" ORDER BY m.created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<MovementRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(StockMovementListResponse {
        items: rows.into_iter().map(MovementRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/inventory/movements
#[utoipa::path(post, path = "/api/v1/inventory/movements", tag = "inventory-movements",
    request_body = CreateStockMovementRequest, responses((status = 201, body = StockMovementDto)))]
pub async fn create_movement(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateStockMovementRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_stock_move(),
        &request_id,
    )?;

    let warehouse_id = super::parse_public_id(IdKind::Warehouse, &body.warehouse_id, &request_id)?;
    let item_id = super::parse_public_id(IdKind::InventoryItem, &body.item_id, &request_id)?;
    let source_id = parse_optional_public_id_any(body.source_id.as_deref(), &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let outcome = stock::post_movement(
        &mut tx,
        auth.ctx.org_id,
        &auth.ctx.actor,
        PostMovementInput {
            warehouse_id,
            item_id,
            qty_delta: body.qty_delta,
            unit_cost_minor: body.unit_cost_minor.unwrap_or(0),
            movement_type: body.movement_type.clone(),
            source_type: body.source_type.clone(),
            source_id,
            idempotency_key: idem_key.clone(),
            memo: body.memo.clone(),
            created_by: auth.ctx.actor.user_id,
        },
        &request_id,
    )
    .await?;

    // Issue / transfer-out movements recognize COGS immediately (Dr COGS /
    // Cr Inventory) — best-effort: the movement itself is already committed
    // to the ledger, so a finance outage here is logged but does not roll
    // back the stock movement (matches HR payroll's posture toward finance).
    let mut cogs_journal_public_id = None;
    if let Some(cogs_minor) = outcome.cogs_minor {
        if cogs_minor > 0 && !outcome.replayed {
            match finance_client::post_cogs_journal(
                &auth,
                outcome.movement_id,
                &outcome.currency,
                cogs_minor,
                format!("COGS for movement {}", outcome.movement_public_id),
                idem_key.as_deref(),
                &request_id,
            )
            .await
            {
                Ok(jid) => cogs_journal_public_id = Some(jid),
                Err(e) => {
                    tracing::warn!(error = %e.detail, "failed to post COGS journal for issue movement");
                }
            }
        }
    }

    let dto = StockMovementDto {
        id: outcome.movement_public_id,
        warehouse_id: body.warehouse_id,
        item_id: body.item_id,
        qty_delta: body.qty_delta,
        unit_cost_minor: body.unit_cost_minor.unwrap_or(0),
        currency: outcome.currency,
        movement_type: body.movement_type,
        source_type: body.source_type,
        source_id: source_id.map(|u| u.to_string()),
        memo: body.memo,
        created_at: Utc::now().to_rfc3339(),
        cogs_journal_public_id,
        qty_on_hand_after: outcome.qty_on_hand,
        avg_unit_cost_minor_after: outcome.avg_unit_cost_minor,
        low_stock: outcome.low_stock,
    };

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

fn parse_optional_public_id_any(
    raw: Option<&str>,
    request_id: &str,
) -> Result<Option<Uuid>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => {
            if let Ok(u) = Uuid::parse_str(s) {
                return Ok(Some(u));
            }
            let pid: PublicId = s
                .parse()
                .map_err(|_| validation(request_id, format!("invalid source_id: {s}")))?;
            Ok(Some(pid.uuid()))
        }
    }
}

/// POST /api/v1/inventory/stock/reconcile
#[utoipa::path(post, path = "/api/v1/inventory/stock/reconcile", tag = "inventory-movements",
    request_body = ReconcileStockRequest, responses((status = 200, body = ReconcileStockResponse)))]
pub async fn reconcile_stock(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ReconcileStockRequest>,
) -> Result<Json<ReconcileStockResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::inventory_stock_move(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    let warehouse_id =
        parse_optional_public_id(IdKind::Warehouse, body.warehouse_id.as_deref(), &request_id)?;
    let item_id =
        parse_optional_public_id(IdKind::InventoryItem, body.item_id.as_deref(), &request_id)?;

    let (checked, outcomes) = match (warehouse_id, item_id) {
        (Some(w), Some(i)) => {
            let outcome = stock::reconcile_stock(
                &mut tx,
                auth.ctx.org_id,
                &auth.ctx.actor,
                w,
                i,
                &request_id,
            )
            .await?;
            (1, outcome.into_iter().collect::<Vec<_>>())
        }
        _ => {
            let outcomes =
                stock::reconcile_all(&mut tx, auth.ctx.org_id, &auth.ctx.actor, &request_id)
                    .await?;
            (outcomes.len() as i64, outcomes)
        }
    };

    let mut alerts = Vec::with_capacity(outcomes.len());
    for o in outcomes {
        alerts.push(DriftAlertDto {
            id: o.id.to_string(),
            warehouse_id: PublicId::new(IdKind::Warehouse, o.warehouse_id).as_str(),
            item_id: PublicId::new(IdKind::InventoryItem, o.item_id).as_str(),
            cached_qty: o.cached_qty,
            movement_sum_qty: o.movement_sum_qty,
            detected_at: o.detected_at.to_rfc3339(),
        });
    }

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ReconcileStockResponse {
        checked,
        drift_count: alerts.len() as i64,
        alerts,
    }))
}
