//! Stub document extractor — heuristic key=value and amount parsing.

use companyos_ids::new_uuid_v7;
use companyos_tenancy::set_session_org_id;
use serde_json::json;
use uuid::Uuid;

use crate::provider::wrap_untrusted;
use crate::state::AppState;
use crate::types::{DocumentExtractRequest, DocumentReview};
use companyos_errors::{AppError, ErrorCode};

#[derive(Debug, Clone)]
pub struct ExtractedFields {
    pub amount_minor: i64,
    pub currency: String,
    pub vendor: Option<String>,
    pub date: Option<String>,
    pub confidence: f64,
    pub raw_wrapped: String,
}

pub fn extract_from_text(text: &str, _kind: &str) -> ExtractedFields {
    let wrapped = wrap_untrusted(text);
    let lower = text.to_ascii_lowercase();
    let mut amount_minor = 0i64;
    let mut currency = "USD".to_string();
    let mut vendor: Option<String> = None;
    let mut date: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim();
            match key.as_str() {
                "amount" | "total" => {
                    amount_minor = parse_amount_minor(val).unwrap_or(amount_minor);
                }
                "currency" => currency = val.to_uppercase(),
                "vendor" | "supplier" => vendor = Some(val.to_string()),
                "date" => date = Some(val.to_string()),
                _ => {}
            }
        }
        if trimmed.to_ascii_lowercase().starts_with("total:") {
            let rest = trimmed.split(':').nth(1).unwrap_or("").trim();
            amount_minor = parse_amount_minor(rest).unwrap_or(amount_minor);
        }
    }

    if amount_minor == 0 {
        amount_minor = heuristic_amount(&lower).unwrap_or(0);
    }

    let confidence = if amount_minor > 0 && vendor.is_some() {
        0.85
    } else if amount_minor > 0 {
        0.70
    } else {
        0.55
    };

    ExtractedFields {
        amount_minor,
        currency,
        vendor,
        date,
        confidence,
        raw_wrapped: wrapped,
    }
}

fn parse_amount_minor(s: &str) -> Option<i64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    if let Ok(f) = cleaned.parse::<f64>() {
        return Some((f * 100.0).round() as i64);
    }
    None
}

fn heuristic_amount(lower: &str) -> Option<i64> {
    if let Some(idx) = lower.find("total:") {
        let rest = lower[idx + 6..].trim();
        return parse_amount_minor(rest);
    }
    if let Some(idx) = lower.find('$') {
        let rest = lower[idx..].trim();
        return parse_amount_minor(rest);
    }
    None
}

pub fn build_proposal_command(
    kind: &str,
    fields: &ExtractedFields,
) -> (String, serde_json::Value, String) {
    match kind {
        "invoice" => {
            let cmd = json!({
                "customer_id": "cus_placeholder",
                "currency": fields.currency,
                "lines": [{
                    "description": fields.vendor.clone().unwrap_or_else(|| "Invoice line".into()),
                    "quantity": 1,
                    "unit_price_minor": fields.amount_minor,
                }],
            });
            let diff = format!(
                "+ Invoice draft\n  Amount: {} {} minor",
                fields.currency, fields.amount_minor
            );
            ("create_invoice".into(), cmd, diff)
        }
        _ => {
            let cmd = json!({
                "currency": fields.currency,
                "amount_minor": fields.amount_minor,
                "description": fields.vendor.clone().unwrap_or_else(|| "Expense".into()),
                "incurred_at": fields.date.clone(),
            });
            let diff = format!(
                "+ Expense draft\n  Amount: {} {} minor",
                fields.currency, fields.amount_minor
            );
            ("create_expense".into(), cmd, diff)
        }
    }
}

pub async fn persist_review(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    req: &DocumentExtractRequest,
    fields: &ExtractedFields,
    proposal_id: Option<Uuid>,
    request_id: &str,
) -> Result<DocumentReview, AppError> {
    let review_id = new_uuid_v7();
    let extracted = json!({
        "amount_minor": fields.amount_minor,
        "currency": fields.currency,
        "vendor": fields.vendor,
        "date": fields.date,
        "raw": fields.raw_wrapped,
    });

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, companyos_tenancy::OrgId::new(org_id))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_document_review
            (id, org_id, user_id, file_id, kind, extractor, confidence, extracted, proposal_id, status)
        VALUES ($1, $2, $3, $4, $5, 'stub', $6, $7, $8, 'pending_review')
        "#,
    )
    .bind(review_id)
    .bind(org_id)
    .bind(user_id)
    .bind(req.file_id.as_deref())
    .bind(&req.kind)
    .bind(fields.confidence)
    .bind(&extracted)
    .bind(proposal_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(DocumentReview {
        id: review_id.to_string(),
        kind: req.kind.clone(),
        confidence: fields.confidence,
        extracted,
        proposal_id: proposal_id.map(|u| u.to_string()),
        status: "pending_review".into(),
    })
}
