//! HTTP client for `companyos-finance` — posting journals for the three
//! inventory-owned source types (`inventory_receipt`, `inventory_cogs`,
//! `inventory_depreciation`) and driving the procure-to-pay vendor-bill
//! endpoints. Inventory-service never writes `finance_*` tables directly.

use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::AuthCtx;

/// Ledger account codes used by inventory journals (see `finance-service`
/// `journal::codes` — kept in sync with `ensure_ledger_accounts`).
pub mod codes {
    pub const INVENTORY: &str = "1200";
    pub const AP_VENDORS: &str = "2000";
    pub const COGS: &str = "5200";
    pub const DEPRECIATION_EXPENSE: &str = "5300";
    pub const ACCUMULATED_DEPRECIATION: &str = "1300";
}

fn finance_base_url() -> String {
    std::env::var("FINANCE_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8083".into())
}

/// Map an upstream finance-service HTTP status to the closest local
/// `ErrorCode` so callers (and their HTTP responses) reflect *why* the
/// journal was rejected — e.g. posting to a closed fiscal period comes back
/// as 409 from finance and should surface as `Conflict` here too, not a
/// generic 500.
fn map_upstream_status(status: reqwest::StatusCode) -> ErrorCode {
    match status.as_u16() {
        409 => ErrorCode::Conflict,
        400 | 422 => ErrorCode::ValidationFailed,
        403 => ErrorCode::Forbidden,
        404 => ErrorCode::NotFound,
        _ => ErrorCode::Internal,
    }
}

fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::new(ErrorCode::Internal, "unknown", e.to_string()))
}

fn with_actor_headers(
    req: reqwest::RequestBuilder,
    auth: &AuthCtx,
    idem_key: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = req
        .header(
            "x-companyos-dev-org-id",
            auth.ctx.org_id.to_public().as_str(),
        )
        .header(
            "x-companyos-dev-user-id",
            PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
        )
        .header(
            "x-companyos-on-behalf-of",
            PublicId::new(IdKind::User, auth.ctx.actor.on_behalf_of).as_str(),
        );
    if let Some(key) = idem_key {
        req = req.header("idempotency-key", key);
    }
    req
}

/// Post a balanced two-line journal to `/api/v1/finance/journals`. Returns
/// the finance-assigned journal public id (`jrn_…`).
#[allow(clippy::too_many_arguments)]
async fn post_journal_lines(
    auth: &AuthCtx,
    source_type: &str,
    source_id: Uuid,
    currency: &str,
    memo: String,
    debit_account: &str,
    credit_account: &str,
    amount_minor: i64,
    idem_key: Option<&str>,
    request_id: &str,
) -> Result<String, AppError> {
    let url = format!("{}/api/v1/finance/journals", finance_base_url().trim_end_matches('/'));
    let lines = vec![
        serde_json::json!({
            "account_code": debit_account,
            "debit_minor": amount_minor,
            "credit_minor": 0,
            "memo": memo,
        }),
        serde_json::json!({
            "account_code": credit_account,
            "debit_minor": 0,
            "credit_minor": amount_minor,
            "memo": memo,
        }),
    ];

    let c = client()?;
    let req = c.post(&url).json(&serde_json::json!({
        "source_type": source_type,
        "source_id": source_id.to_string(),
        "currency": currency,
        "memo": memo,
        "lines": lines,
    }));
    let req = with_actor_headers(req, auth, idem_key);

    let resp = req.send().await.map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("finance journal request failed: {e}"),
        )
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::new(
            map_upstream_status(status),
            request_id,
            format!("finance journal post failed: {status} {body}"),
        ));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    body.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id,
                "finance journal response missing id",
            )
        })
}

/// Dr Inventory (1200) / Cr AP — Vendors (2000). Posted when a goods receipt
/// is posted (GRN → finance liability at the received cost).
pub async fn post_receipt_journal(
    auth: &AuthCtx,
    source_id: Uuid,
    currency: &str,
    amount_minor: i64,
    memo: String,
    idem_key: Option<&str>,
    request_id: &str,
) -> Result<String, AppError> {
    post_journal_lines(
        auth,
        "inventory_receipt",
        source_id,
        currency,
        memo,
        codes::INVENTORY,
        codes::AP_VENDORS,
        amount_minor,
        idem_key,
        request_id,
    )
    .await
}

/// Dr COGS (5200) / Cr Inventory (1200). Posted when stock is issued.
pub async fn post_cogs_journal(
    auth: &AuthCtx,
    source_id: Uuid,
    currency: &str,
    amount_minor: i64,
    memo: String,
    idem_key: Option<&str>,
    request_id: &str,
) -> Result<String, AppError> {
    post_journal_lines(
        auth,
        "inventory_cogs",
        source_id,
        currency,
        memo,
        codes::COGS,
        codes::INVENTORY,
        amount_minor,
        idem_key,
        request_id,
    )
    .await
}

/// Dr Depreciation Expense (5300) / Cr Accumulated Depreciation (1300).
/// Posted by the asset depreciation endpoint.
pub async fn post_depreciation_journal(
    auth: &AuthCtx,
    source_id: Uuid,
    currency: &str,
    amount_minor: i64,
    memo: String,
    idem_key: Option<&str>,
    request_id: &str,
) -> Result<String, AppError> {
    post_journal_lines(
        auth,
        "inventory_depreciation",
        source_id,
        currency,
        memo,
        codes::DEPRECIATION_EXPENSE,
        codes::ACCUMULATED_DEPRECIATION,
        amount_minor,
        idem_key,
        request_id,
    )
    .await
}

/// Minimal projection of finance-service's `VendorBillDto` — only the
/// fields inventory-service needs to surface back to its own caller.
#[derive(Debug, Clone, Deserialize)]
pub struct VendorBillClientDto {
    pub id: String,
    pub supplier_ref: String,
    pub source_type: String,
    pub source_id: Option<String>,
    pub currency: String,
    pub amount_minor: i64,
    pub amount_paid_minor: i64,
    pub status: String,
    pub payment_journal_public_id: Option<String>,
}

/// POST `/api/v1/finance/vendor-bills` — records the AP liability against a
/// specific PO/GRN reference. Does **not** post a journal on finance's side
/// (AP is already booked at receipt time via [`post_receipt_journal`]).
#[allow(clippy::too_many_arguments)]
pub async fn create_vendor_bill(
    auth: &AuthCtx,
    supplier_ref: &str,
    source_type: &str,
    source_id: Option<&str>,
    currency: &str,
    amount_minor: i64,
    memo: Option<&str>,
    idem_key: Option<&str>,
    request_id: &str,
) -> Result<VendorBillClientDto, AppError> {
    let url = format!(
        "{}/api/v1/finance/vendor-bills",
        finance_base_url().trim_end_matches('/')
    );
    let c = client()?;
    let req = c.post(&url).json(&serde_json::json!({
        "supplier_ref": supplier_ref,
        "source_type": source_type,
        "source_id": source_id,
        "currency": currency,
        "amount_minor": amount_minor,
        "memo": memo,
    }));
    let req = with_actor_headers(req, auth, idem_key);
    let resp = req.send().await.map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("finance vendor-bill create request failed: {e}"),
        )
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::new(
            map_upstream_status(status),
            request_id,
            format!("finance vendor-bill create failed: {status} {body}"),
        ));
    }
    resp.json()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))
}

/// POST `/api/v1/finance/vendor-bills/{id}/pay` — Dr AP / Cr Cash for
/// `amount_minor` (defaults to the full outstanding balance).
pub async fn pay_vendor_bill(
    auth: &AuthCtx,
    bill_public_id: &str,
    amount_minor: Option<i64>,
    memo: Option<&str>,
    idem_key: Option<&str>,
    request_id: &str,
) -> Result<VendorBillClientDto, AppError> {
    let url = format!(
        "{}/api/v1/finance/vendor-bills/{}/pay",
        finance_base_url().trim_end_matches('/'),
        bill_public_id
    );
    let c = client()?;
    let req = c.post(&url).json(&serde_json::json!({
        "amount_minor": amount_minor,
        "memo": memo,
    }));
    let req = with_actor_headers(req, auth, idem_key);
    let resp = req.send().await.map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("finance vendor-bill pay request failed: {e}"),
        )
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::new(
            map_upstream_status(status),
            request_id,
            format!("finance vendor-bill pay failed: {status} {body}"),
        ));
    }
    resp.json()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))
}
