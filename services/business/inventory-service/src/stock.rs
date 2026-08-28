//! Stock ledger orchestration: append-only movements + the `inventory_stock_level`
//! cache, negative-stock policy, reorder-point alerts, and drift reconciliation.
//!
//! **Source of truth is `inventory_stock_movement`.** `inventory_stock_level`
//! is a derived cache updated in lock-step with every movement insert in the
//! same transaction. [`reconcile_stock`] compares the two and — on
//! mismatch — inserts a `inventory_drift_alert` row plus a
//! `stock.drift_detected` outbox event. It never silently rewrites the
//! cache; an operator (or a follow-up adjustment movement) must resolve it.

use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{Actor, OrgId};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::types::MOVEMENT_TYPES;
use crate::valuation;

/// Movements that increase quantity on hand (qty_delta must be > 0).
const INCREASING_TYPES: &[&str] = &["receipt", "return", "transfer_in"];
/// Movements that decrease quantity on hand (qty_delta must be < 0).
const DECREASING_TYPES: &[&str] = &["issue", "transfer_out"];

pub struct PostMovementInput {
    pub warehouse_id: Uuid,
    pub item_id: Uuid,
    pub qty_delta: i64,
    pub unit_cost_minor: i64,
    pub movement_type: String,
    pub source_type: Option<String>,
    pub source_id: Option<Uuid>,
    pub idempotency_key: Option<String>,
    pub memo: Option<String>,
    pub created_by: Uuid,
}

pub struct PostMovementOutcome {
    pub movement_id: Uuid,
    pub movement_public_id: String,
    pub currency: String,
    pub qty_on_hand: i64,
    pub avg_unit_cost_minor: i64,
    /// Set for issue/transfer_out movements — the caller is responsible for
    /// posting the matching Dr COGS / Cr Inventory journal to finance-service.
    pub cogs_minor: Option<i64>,
    pub low_stock: bool,
    /// True when an existing movement with the same idempotency key was
    /// found and replayed instead of inserting a new one.
    pub replayed: bool,
}

#[derive(sqlx::FromRow)]
struct ItemPolicyRow {
    currency: String,
    reorder_point_qty: i64,
    allow_negative_stock: bool,
    is_active: bool,
}

#[derive(sqlx::FromRow)]
struct StockLevelRow {
    qty_on_hand: i64,
    avg_unit_cost_minor: i64,
}

#[derive(sqlx::FromRow)]
struct ExistingMovementRow {
    id: Uuid,
    public_id: String,
    warehouse_id: Uuid,
    item_id: Uuid,
    currency: String,
}

async fn fetch_item_policy(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    item_id: Uuid,
    request_id: &str,
) -> Result<ItemPolicyRow, AppError> {
    sqlx::query_as(
        r#"
        SELECT currency, reorder_point_qty, allow_negative_stock, is_active
        FROM inventory_item
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(item_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?
    .ok_or_else(|| AppError::new(ErrorCode::NotFound, request_id, "item not found"))
}

async fn fetch_warehouse_exists(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    warehouse_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT is_active FROM inventory_warehouse WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .bind(warehouse_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;
    if row.is_none() {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "warehouse not found",
        ));
    }
    Ok(())
}

async fn fetch_stock_level(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    warehouse_id: Uuid,
    item_id: Uuid,
    request_id: &str,
) -> Result<StockLevelRow, AppError> {
    let row: Option<StockLevelRow> = sqlx::query_as(
        r#"
        SELECT qty_on_hand, avg_unit_cost_minor
        FROM inventory_stock_level
        WHERE org_id = $1 AND warehouse_id = $2 AND item_id = $3
        "#,
    )
    .bind(org_id)
    .bind(warehouse_id)
    .bind(item_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;
    Ok(row.unwrap_or(StockLevelRow {
        qty_on_hand: 0,
        avg_unit_cost_minor: 0,
    }))
}

async fn fetch_existing_by_idem_key(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    key: &str,
    request_id: &str,
) -> Result<Option<ExistingMovementRow>, AppError> {
    sqlx::query_as(
        r#"
        SELECT id, public_id, warehouse_id, item_id, currency
        FROM inventory_stock_movement
        WHERE org_id = $1 AND idempotency_key = $2
        "#,
    )
    .bind(org_id)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))
}

/// Insert one append-only movement, update the stock-level cache, enforce
/// the negative-stock policy, evaluate the reorder point, and insert the
/// `stock.movement_recorded` (+ optional `stock.low`) outbox event(s) — all
/// in the caller's open transaction.
pub async fn post_movement(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    actor: &Actor,
    input: PostMovementInput,
    request_id: &str,
) -> Result<PostMovementOutcome, AppError> {
    let org_uuid = org_id.as_uuid();

    if !MOVEMENT_TYPES.contains(&input.movement_type.as_str()) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("movement_type must be one of {MOVEMENT_TYPES:?}"),
        ));
    }
    if input.qty_delta == 0 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "qty_delta must be non-zero",
        ));
    }
    if INCREASING_TYPES.contains(&input.movement_type.as_str()) && input.qty_delta <= 0 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("{} requires a positive qty_delta", input.movement_type),
        ));
    }
    if DECREASING_TYPES.contains(&input.movement_type.as_str()) && input.qty_delta >= 0 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("{} requires a negative qty_delta", input.movement_type),
        ));
    }

    if let Some(key) = input.idempotency_key.as_deref() {
        if let Some(existing) = fetch_existing_by_idem_key(tx, org_uuid, key, request_id).await? {
            let level = fetch_stock_level(
                tx,
                org_uuid,
                existing.warehouse_id,
                existing.item_id,
                request_id,
            )
            .await?;
            return Ok(PostMovementOutcome {
                movement_id: existing.id,
                movement_public_id: existing.public_id,
                currency: existing.currency,
                qty_on_hand: level.qty_on_hand,
                avg_unit_cost_minor: level.avg_unit_cost_minor,
                cogs_minor: None,
                low_stock: false,
                replayed: true,
            });
        }
    }

    fetch_warehouse_exists(tx, org_uuid, input.warehouse_id, request_id).await?;
    let item = fetch_item_policy(tx, org_uuid, input.item_id, request_id).await?;
    if !item.is_active {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            "item is not active",
        ));
    }

    let level = fetch_stock_level(tx, org_uuid, input.warehouse_id, input.item_id, request_id).await?;

    let (new_qty, new_avg, cogs_minor) = if input.qty_delta > 0 {
        let (q, a) = valuation::weighted_average_receipt(
            level.qty_on_hand,
            level.avg_unit_cost_minor,
            input.qty_delta,
            input.unit_cost_minor,
        )
        .map_err(|e| AppError::new(ErrorCode::ValidationFailed, request_id, e.to_string()))?;
        (q, a, None)
    } else {
        let issued_qty = input.qty_delta.unsigned_abs() as i64;
        let cogs = valuation::issue_cost_minor(issued_qty, level.avg_unit_cost_minor)
            .map_err(|e| AppError::new(ErrorCode::ValidationFailed, request_id, e.to_string()))?;
        let new_qty = level
            .qty_on_hand
            .checked_add(input.qty_delta)
            .ok_or_else(|| AppError::new(ErrorCode::Internal, request_id, "qty overflow"))?;
        (new_qty, level.avg_unit_cost_minor, Some(cogs))
    };

    if new_qty < 0 && !item.allow_negative_stock {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            format!(
                "movement would drive stock negative ({new_qty}) and item does not allow negative stock"
            ),
        ));
    }

    let movement_id = new_uuid_v7();
    let movement_public_id = PublicId::new(IdKind::StockMovement, movement_id).as_str();

    sqlx::query(
        r#"
        INSERT INTO inventory_stock_movement (
            id, org_id, public_id, warehouse_id, item_id, qty_delta, unit_cost_minor,
            currency, movement_type, source_type, source_id, idempotency_key, memo, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
        "#,
    )
    .bind(movement_id)
    .bind(org_uuid)
    .bind(&movement_public_id)
    .bind(input.warehouse_id)
    .bind(input.item_id)
    .bind(input.qty_delta)
    .bind(input.unit_cost_minor)
    .bind(&item.currency)
    .bind(&input.movement_type)
    .bind(input.source_type.as_deref())
    .bind(input.source_id)
    .bind(input.idempotency_key.as_deref())
    .bind(input.memo.as_deref())
    .bind(input.created_by)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;

    sqlx::query(
        r#"
        INSERT INTO inventory_stock_level (org_id, warehouse_id, item_id, qty_on_hand, avg_unit_cost_minor, last_movement_at, updated_at)
        VALUES ($1,$2,$3,$4,$5,now(),now())
        ON CONFLICT (org_id, warehouse_id, item_id) DO UPDATE SET
            qty_on_hand = EXCLUDED.qty_on_hand,
            avg_unit_cost_minor = EXCLUDED.avg_unit_cost_minor,
            last_movement_at = now(),
            updated_at = now()
        "#,
    )
    .bind(org_uuid)
    .bind(input.warehouse_id)
    .bind(input.item_id)
    .bind(new_qty)
    .bind(new_avg)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;

    let low_stock = new_qty <= item.reorder_point_qty;

    let envelope = EventEnvelope::new(
        org_id,
        Context::Inventory,
        "stock",
        "movement_recorded",
        1,
        actor.clone(),
        serde_json::json!({
            "id": movement_public_id,
            "warehouse_id": PublicId::new(IdKind::Warehouse, input.warehouse_id).as_str(),
            "item_id": PublicId::new(IdKind::InventoryItem, input.item_id).as_str(),
            "qty_delta": input.qty_delta,
            "movement_type": input.movement_type,
            "qty_on_hand": new_qty,
            "avg_unit_cost_minor": new_avg,
        }),
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    if low_stock {
        let low_envelope = EventEnvelope::new(
            org_id,
            Context::Inventory,
            "stock",
            "low",
            1,
            actor.clone(),
            serde_json::json!({
                "id": movement_public_id,
                "warehouse_id": PublicId::new(IdKind::Warehouse, input.warehouse_id).as_str(),
                "item_id": PublicId::new(IdKind::InventoryItem, input.item_id).as_str(),
                "qty_on_hand": new_qty,
                "reorder_point_qty": item.reorder_point_qty,
            }),
        );
        companyos_outbox::insert_event(&mut **tx, &low_envelope)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    }

    Ok(PostMovementOutcome {
        movement_id,
        movement_public_id,
        currency: item.currency,
        qty_on_hand: new_qty,
        avg_unit_cost_minor: new_avg,
        cogs_minor,
        low_stock,
        replayed: false,
    })
}

/// Sum of all posted movements for `(warehouse_id, item_id)` — the ledger's
/// view of quantity on hand, independent of the `inventory_stock_level` cache.
pub async fn qty_on_hand_from_movements(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    warehouse_id: Uuid,
    item_id: Uuid,
    request_id: &str,
) -> Result<i64, AppError> {
    let row: (Option<i64>,) = sqlx::query_as(
        r#"
        SELECT SUM(qty_delta)::bigint
        FROM inventory_stock_movement
        WHERE org_id = $1 AND warehouse_id = $2 AND item_id = $3
        "#,
    )
    .bind(org_id)
    .bind(warehouse_id)
    .bind(item_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;
    Ok(row.0.unwrap_or(0))
}

pub struct DriftOutcome {
    pub id: Uuid,
    pub warehouse_id: Uuid,
    pub item_id: Uuid,
    pub cached_qty: i64,
    pub movement_sum_qty: i64,
    pub detected_at: chrono::DateTime<chrono::Utc>,
}

/// Compare the cache against the ledger for one `(warehouse_id, item_id)`
/// pair. On mismatch, insert a `inventory_drift_alert` row + a
/// `stock.drift_detected` outbox event and return it — the cache is left
/// untouched either way.
pub async fn reconcile_stock(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    actor: &Actor,
    warehouse_id: Uuid,
    item_id: Uuid,
    request_id: &str,
) -> Result<Option<DriftOutcome>, AppError> {
    let org_uuid = org_id.as_uuid();
    let level = fetch_stock_level(tx, org_uuid, warehouse_id, item_id, request_id).await?;
    let movement_sum = qty_on_hand_from_movements(tx, org_uuid, warehouse_id, item_id, request_id).await?;

    if level.qty_on_hand == movement_sum {
        return Ok(None);
    }

    let alert_id = new_uuid_v7();
    let row: (chrono::DateTime<chrono::Utc>,) = sqlx::query_as(
        r#"
        INSERT INTO inventory_drift_alert (
            id, org_id, warehouse_id, item_id, cached_qty, movement_sum_qty
        ) VALUES ($1,$2,$3,$4,$5,$6)
        RETURNING detected_at
        "#,
    )
    .bind(alert_id)
    .bind(org_uuid)
    .bind(warehouse_id)
    .bind(item_id)
    .bind(level.qty_on_hand)
    .bind(movement_sum)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;

    let envelope = EventEnvelope::new(
        org_id,
        Context::Inventory,
        "stock",
        "drift_detected",
        1,
        actor.clone(),
        serde_json::json!({
            "id": alert_id.to_string(),
            "warehouse_id": PublicId::new(IdKind::Warehouse, warehouse_id).as_str(),
            "item_id": PublicId::new(IdKind::InventoryItem, item_id).as_str(),
            "cached_qty": level.qty_on_hand,
            "movement_sum_qty": movement_sum,
        }),
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(Some(DriftOutcome {
        id: alert_id,
        warehouse_id,
        item_id,
        cached_qty: level.qty_on_hand,
        movement_sum_qty: movement_sum,
        detected_at: row.0,
    }))
}

/// Reconcile every `(warehouse_id, item_id)` pair known to either the cache
/// or the movement ledger for this org.
pub async fn reconcile_all(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    actor: &Actor,
    request_id: &str,
) -> Result<Vec<DriftOutcome>, AppError> {
    let org_uuid = org_id.as_uuid();
    let pairs: Vec<(Uuid, Uuid)> = sqlx::query_as(
        r#"
        SELECT warehouse_id, item_id FROM inventory_stock_level WHERE org_id = $1
        UNION
        SELECT warehouse_id, item_id FROM inventory_stock_movement WHERE org_id = $1
        "#,
    )
    .bind(org_uuid)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;

    let mut outcomes = Vec::new();
    for (warehouse_id, item_id) in pairs {
        if let Some(outcome) =
            reconcile_stock(tx, org_id, actor, warehouse_id, item_id, request_id).await?
        {
            outcomes.push(outcome);
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increasing_and_decreasing_type_lists_are_disjoint_and_cover_signed_types() {
        for t in INCREASING_TYPES {
            assert!(!DECREASING_TYPES.contains(t));
        }
        // adjustment is intentionally excluded from both — either sign allowed.
        assert!(!INCREASING_TYPES.contains(&"adjustment"));
        assert!(!DECREASING_TYPES.contains(&"adjustment"));
    }
}
