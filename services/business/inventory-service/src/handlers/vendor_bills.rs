//! `/api/v1/inventory/procure-to-pay/vendor-bill…` — thin proxy in front of
//! `companyos-finance`'s vendor-bill endpoints, closing the procure-to-pay
//! loop (PR → PO → GRN → vendor bill → payment) without inventory-service
//! ever writing a `finance_*` table itself.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::IdKind;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{conflict, internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::finance_client::{self, VendorBillClientDto};
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{CreateVendorBillFromReceiptRequest, PayVendorBillRequest, VendorBillProxyDto};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inventory/procure-to-pay/vendor-bill",
            axum::routing::post(create_vendor_bill_from_receipt),
        )
        .route(
            "/api/v1/inventory/procure-to-pay/vendor-bill/{id}/pay",
            axum::routing::post(pay_vendor_bill),
        )
}

fn to_dto(bill: VendorBillClientDto) -> VendorBillProxyDto {
    VendorBillProxyDto {
        id: bill.id,
        supplier_ref: bill.supplier_ref,
        source_type: bill.source_type,
        source_id: bill.source_id,
        currency: bill.currency,
        amount_minor: bill.amount_minor,
        amount_paid_minor: bill.amount_paid_minor,
        status: bill.status,
        payment_journal_public_id: bill.payment_journal_public_id,
    }
}

/// POST /api/v1/inventory/procure-to-pay/vendor-bill
///
/// Records the AP liability for a *posted* goods receipt against
/// `supplier_ref` (finance-service's own supplier reference — inventory
/// does not own the AP supplier master). The bill amount is the sum of the
/// GRN's line values (`qty_received * unit_cost_minor`), i.e. exactly the
/// value already booked to Inventory/AP via [`finance_client::post_receipt_journal`]
/// when the GRN was posted.
#[utoipa::path(post, path = "/api/v1/inventory/procure-to-pay/vendor-bill", tag = "inventory-procure-to-pay",
    request_body = CreateVendorBillFromReceiptRequest, responses((status = 201, body = VendorBillProxyDto)))]
pub async fn create_vendor_bill_from_receipt(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateVendorBillFromReceiptRequest>,
) -> Result<(StatusCode, Json<VendorBillProxyDto>), AppError> {
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
        perms::inventory_goods_receipt_write(),
        &request_id,
    )?;

    if body.supplier_ref.trim().is_empty() {
        return Err(validation(&request_id, "supplier_ref is required"));
    }
    let grn_id = parse_public_id(IdKind::GoodsReceipt, &body.goods_receipt_id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;

    #[derive(sqlx::FromRow)]
    struct GrnForBill {
        id: Uuid,
        public_id: String,
        status: String,
        currency: String,
    }
    let grn: Option<GrnForBill> = sqlx::query_as(
        r#"
        SELECT g.id, g.public_id, g.status, po.currency
        FROM inventory_goods_receipt g
        JOIN inventory_purchase_order po ON po.id = g.purchase_order_id AND po.org_id = g.org_id
        WHERE g.org_id = $1 AND g.id = $2
        "#,
    )
    .bind(org_id)
    .bind(grn_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let Some(grn) = grn else {
        return Err(not_found(&request_id, "goods receipt"));
    };
    if grn.status != "posted" {
        return Err(conflict(
            &request_id,
            format!("goods receipt status {} is not posted", grn.status),
        ));
    }

    let total: (Option<i64>,) = sqlx::query_as(
        r#"
        SELECT SUM(qty_received * unit_cost_minor)::bigint
        FROM inventory_goods_receipt_line
        WHERE org_id = $1 AND receipt_id = $2
        "#,
    )
    .bind(org_id)
    .bind(grn.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let amount_minor = total.0.unwrap_or(0);
    if amount_minor <= 0 {
        return Err(conflict(&request_id, "goods receipt has no receipted value"));
    }

    let bill = finance_client::create_vendor_bill(
        &auth,
        body.supplier_ref.trim(),
        "goods_receipt",
        Some(&grn.public_id),
        &grn.currency,
        amount_minor,
        body.memo.as_deref(),
        idem_key.as_deref(),
        &request_id,
    )
    .await?;
    let dto = to_dto(bill);

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.vendor_bill.create",
        "goods_receipt",
        &grn.public_id,
        serde_json::json!({ "vendor_bill_id": dto.id, "amount_minor": dto.amount_minor }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// POST /api/v1/inventory/procure-to-pay/vendor-bill/{id}/pay
///
/// `{id}` is finance-service's own vendor-bill public id (`vb_…`) — the id
/// returned by [`create_vendor_bill_from_receipt`]. Dr AP / Cr Cash is
/// posted entirely on finance's side; inventory-service only forwards the
/// request and relays the resulting bill state back to its own caller.
#[utoipa::path(post, path = "/api/v1/inventory/procure-to-pay/vendor-bill/{id}/pay", tag = "inventory-procure-to-pay",
    params(("id" = String, Path)), request_body = PayVendorBillRequest,
    responses((status = 200, body = VendorBillProxyDto)))]
pub async fn pay_vendor_bill(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<PayVendorBillRequest>,
) -> Result<Json<VendorBillProxyDto>, AppError> {
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
        perms::inventory_goods_receipt_write(),
        &request_id,
    )?;

    if let Some(amount) = body.amount_minor {
        if amount <= 0 {
            return Err(validation(&request_id, "amount_minor must be > 0"));
        }
    }
    let idem_key = idempotency::header_key(&headers);

    let bill = finance_client::pay_vendor_bill(
        &auth,
        &id,
        body.amount_minor,
        body.memo.as_deref(),
        idem_key.as_deref(),
        &request_id,
    )
    .await?;
    let dto = to_dto(bill);

    let org_id = auth.ctx.org_id.as_uuid();
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| internal(&request_id)(sqlx::Error::Protocol(e.to_string())))?;
    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "inventory.vendor_bill.pay",
        "vendor_bill",
        &id,
        serde_json::json!({ "amount_minor": dto.amount_paid_minor, "status": dto.status }),
    )
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(dto))
}
