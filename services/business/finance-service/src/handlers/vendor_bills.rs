//! `/api/v1/finance/vendor-bills` — procure-to-pay support for inventory-service.
//!
//! AP is booked at goods-receipt time via the standard journal endpoint
//! (Dr Inventory / Cr Accounts Payable — Vendors). Creating a bill records
//! that liability against a specific PO/GRN reference **without** posting a
//! journal (it would double-count AP already booked at receipt). Paying a
//! bill posts Dr AP / Cr Cash.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{internal, normalize_paging, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::journal::{codes, ensure_ledger_accounts, post_journal, JournalDraft, LedgerLine};
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreateVendorBillRequest, PayVendorBillRequest, VendorBillDto, VendorBillListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/vendor-bills",
            get(list_vendor_bills).post(create_vendor_bill),
        )
        .route("/api/v1/finance/vendor-bills/{id}", get(get_vendor_bill))
        .route(
            "/api/v1/finance/vendor-bills/{id}/pay",
            post(pay_vendor_bill),
        )
}

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct VendorBillRow {
    public_id: String,
    supplier_ref: String,
    source_type: String,
    source_id: Option<String>,
    currency: String,
    amount_minor: i64,
    amount_paid_minor: i64,
    status: String,
    memo: Option<String>,
    payment_journal_public_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

impl VendorBillRow {
    fn into_dto(self) -> VendorBillDto {
        VendorBillDto {
            id: self.public_id,
            supplier_ref: self.supplier_ref,
            source_type: self.source_type,
            source_id: self.source_id,
            currency: self.currency,
            amount_minor: self.amount_minor,
            amount_paid_minor: self.amount_paid_minor,
            status: self.status,
            memo: self.memo,
            payment_journal_public_id: self.payment_journal_public_id,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

const BILL_COLS: &str = r#"
    public_id, supplier_ref, source_type, source_id, currency,
    amount_minor, amount_paid_minor, status, memo, payment_journal_public_id,
    created_at, updated_at, version
"#;

async fn fetch_bill(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    id: Uuid,
) -> Result<Option<VendorBillRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {BILL_COLS} FROM finance_vendor_bill WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
}

/// GET /api/v1/finance/vendor-bills
#[utoipa::path(get, path = "/api/v1/finance/vendor-bills", tag = "finance-vendor-bills",
    params(
        ("status" = Option<String>, Query),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = VendorBillListResponse)))]
pub async fn list_vendor_bills(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<VendorBillListResponse>, AppError> {
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
    enforce_any_scope(
        &membership.principal,
        perms::finance_journal_post(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM finance_vendor_bill WHERE org_id = ");
    count_qb.push_bind(org_id);
    if let Some(status) = q.status.as_deref() {
        count_qb.push(" AND status = ");
        count_qb.push_bind(status.to_string());
    }
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new(format!("SELECT {BILL_COLS} FROM finance_vendor_bill WHERE org_id = "));
    qb.push_bind(org_id);
    if let Some(status) = q.status.as_deref() {
        qb.push(" AND status = ");
        qb.push_bind(status.to_string());
    }
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<VendorBillRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(VendorBillListResponse {
        items: rows.into_iter().map(VendorBillRow::into_dto).collect(),
        total,
    }))
}

/// GET /api/v1/finance/vendor-bills/{id}
#[utoipa::path(get, path = "/api/v1/finance/vendor-bills/{id}", tag = "finance-vendor-bills",
    params(("id" = String, Path)),
    responses((status = 200, body = VendorBillDto), (status = 404)))]
pub async fn get_vendor_bill(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<VendorBillDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let bill_id = parse_public_id(IdKind::VendorBill, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_journal_post(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let dto = fetch_bill(&mut tx, org_id, bill_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "vendor bill"))?
        .into_dto();
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/finance/vendor-bills
#[utoipa::path(post, path = "/api/v1/finance/vendor-bills", tag = "finance-vendor-bills",
    request_body = CreateVendorBillRequest,
    responses((status = 201, body = VendorBillDto), (status = 200, body = VendorBillDto)))]
pub async fn create_vendor_bill(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateVendorBillRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
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
        perms::finance_journal_post(),
        &request_id,
    )?;

    if body.supplier_ref.trim().is_empty() {
        return Err(validation(&request_id, "supplier_ref is required"));
    }
    if body.amount_minor <= 0 {
        return Err(validation(&request_id, "amount_minor must be positive"));
    }
    let _currency: Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;
    let source_type = body.source_type.as_deref().unwrap_or("goods_receipt");
    if !["goods_receipt", "purchase_order", "manual"].contains(&source_type) {
        return Err(validation(
            &request_id,
            "source_type must be goods_receipt, purchase_order, or manual",
        ));
    }

    let public_id = PublicId::generate(IdKind::VendorBill);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "vendor_bill.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    sqlx::query(
        r#"
        INSERT INTO finance_vendor_bill (
            id, org_id, public_id, supplier_ref, source_type, source_id,
            currency, amount_minor, status, memo, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'open',$9,$10)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.supplier_ref.trim())
    .bind(source_type)
    .bind(body.source_id.as_deref())
    .bind(&body.currency)
    .bind(body.amount_minor)
    .bind(body.memo.as_deref())
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_bill(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "vendor bill missing after insert",
            )
        })?
        .into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "vendor_bill",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "supplier_ref": dto.supplier_ref,
            "amount_minor": dto.amount_minor,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.vendor_bill.create",
        "vendor_bill",
        &dto.id,
        serde_json::json!({ "supplier_ref": dto.supplier_ref, "amount_minor": dto.amount_minor }),
    )
    .await
    .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "vendor_bill.create",
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

/// POST /api/v1/finance/vendor-bills/{id}/pay
#[utoipa::path(post, path = "/api/v1/finance/vendor-bills/{id}/pay", tag = "finance-vendor-bills",
    request_body = PayVendorBillRequest,
    responses((status = 200, body = VendorBillDto), (status = 409)))]
pub async fn pay_vendor_bill(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<PayVendorBillRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let bill_id = parse_public_id(IdKind::VendorBill, &id, &request_id)?;
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
        perms::finance_journal_post(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "vendor_bill.pay", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let row = fetch_bill(&mut tx, org_id, bill_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "vendor bill"))?;

    if row.status == "paid" {
        let dto = row.into_dto();
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((StatusCode::OK, Json(dto)).into_response());
    }
    if row.status == "void" {
        return Err(validation(&request_id, "vendor bill is void"));
    }

    let outstanding = row.amount_minor - row.amount_paid_minor;
    let pay_amount = body.amount_minor.unwrap_or(outstanding);
    if pay_amount <= 0 || pay_amount > outstanding {
        return Err(validation(
            &request_id,
            format!("amount_minor must be between 1 and {outstanding}"),
        ));
    }
    let currency: Currency = row
        .currency
        .parse()
        .map_err(|_| AppError::new(ErrorCode::Internal, request_id.clone(), "bad currency"))?;

    // Dr Accounts Payable — Vendors, Cr Cash.
    let draft = JournalDraft {
        memo: body
            .memo
            .clone()
            .unwrap_or_else(|| format!("Vendor payment for bill {}", row.public_id)),
        source_type: "vendor_payment",
        source_id: bill_id,
        currency,
        entry_date: None,
        reverses_entry_id: None,
        posted_by: Some(auth.ctx.actor.user_id),
        lines: vec![
            LedgerLine::debit(codes::AP_VENDORS, pay_amount, Some("AP settlement".into())),
            LedgerLine::credit(codes::CASH, pay_amount, Some("Cash paid".into())),
        ],
    };
    draft
        .assert_balanced()
        .map_err(|e| validation(&request_id, format!("journal: {e}")))?;
    let journal_entry_id = post_journal(&mut tx, org_id, &draft, &request_id).await?;
    let journal_public_id = format!("jrn_{journal_entry_id}");

    let new_paid = row.amount_paid_minor + pay_amount;
    let new_status = if new_paid >= row.amount_minor {
        "paid"
    } else {
        "partially_paid"
    };

    sqlx::query(
        r#"
        UPDATE finance_vendor_bill SET
            amount_paid_minor = $3, status = $4, payment_journal_public_id = $5,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(bill_id)
    .bind(new_paid)
    .bind(new_status)
    .bind(&journal_public_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_bill(&mut tx, org_id, bill_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "vendor bill"))?
        .into_dto();

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "vendor_bill",
        "paid",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": dto.id,
            "amount_minor": pay_amount,
            "journal_public_id": journal_public_id,
            "status": new_status,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.vendor_bill.pay",
        "vendor_bill",
        &dto.id,
        serde_json::json!({ "amount_minor": pay_amount, "journal_public_id": journal_public_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "vendor_bill.pay",
            key,
            200,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::OK, Json(dto)).into_response())
}
