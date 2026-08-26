//! `/api/v1/finance/credit-notes` — create+issue in one step.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{
    balance_and_status, conflict, internal, not_found, parse_public_id, validation,
};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::invoice_math::{compute_document_totals, LineInput};
use crate::journal::{credit_note_entry, ensure_ledger_accounts, post_journal};
use crate::numbering::next_credit_number;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{CreateCreditNoteRequest, CreditNoteDto};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/finance/credit-notes", post(create_credit_note))
}

/// POST /api/v1/finance/credit-notes — create and issue immediately.
#[utoipa::path(post, path = "/api/v1/finance/credit-notes", tag = "finance-credit-notes",
    request_body = CreateCreditNoteRequest,
    responses((status = 201, body = CreditNoteDto)))]
pub async fn create_credit_note(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateCreditNoteRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);
    let invoice_id = parse_public_id(IdKind::Invoice, &body.invoice_id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_credit_note_create(),
        &request_id,
    )?;

    if body.lines.is_empty() {
        return Err(validation(&request_id, "credit note requires lines"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "credit_note.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let inv: Option<(Uuid, String, String, i64, i64, i64, i64, String)> = sqlx::query_as(
        r#"
        SELECT customer_id, public_id, currency, total_minor, amount_paid_minor,
               amount_credited_minor, balance_minor, status
        FROM finance_invoice
        WHERE org_id = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let Some((
        customer_id,
        inv_public,
        currency_str,
        total,
        paid,
        credited,
        balance,
        status,
    )) = inv
    else {
        return Err(not_found(&request_id, "invoice"));
    };
    if matches!(status.as_str(), "draft" | "void") {
        return Err(conflict(
            &request_id,
            format!("cannot credit invoice in status {status}"),
        ));
    }

    let currency = Currency::new(&currency_str)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let inputs: Vec<LineInput> = body
        .lines
        .iter()
        .map(|l| LineInput {
            quantity: l.quantity,
            unit_price_minor: l.unit_price_minor,
            discount_minor: l.discount_minor,
            tax_rate_bps: l.tax_rate_bps,
        })
        .collect();
    let (computed, doc) = compute_document_totals(&inputs, currency)
        .map_err(|e| validation(&request_id, format!("invalid line totals: {e}")))?;

    if doc.total_minor <= 0 {
        return Err(validation(&request_id, "credit total must be positive"));
    }
    if doc.total_minor > balance {
        return Err(validation(
            &request_id,
            format!(
                "credit total {} exceeds invoice balance {}",
                doc.total_minor, balance
            ),
        ));
    }

    let public_id = PublicId::generate(IdKind::CreditNote);
    let id = public_id.uuid();
    let credit_number = next_credit_number(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;
    let issued_at = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO finance_credit_note (
            id, org_id, public_id, invoice_id, customer_id, owner_user_id, status,
            credit_number, currency, subtotal_minor, tax_minor, total_minor,
            reason, issued_at
        ) VALUES ($1,$2,$3,$4,$5,$6,'issued',$7,$8,$9,$10,$11,$12,$13)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(invoice_id)
    .bind(customer_id)
    .bind(auth.ctx.actor.user_id)
    .bind(&credit_number)
    .bind(&currency_str)
    .bind(doc.subtotal_minor - doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .bind(&body.reason)
    .bind(issued_at)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for (line, totals) in body.lines.iter().zip(computed.iter()) {
        sqlx::query(
            r#"
            INSERT INTO finance_credit_note_line (
                id, org_id, credit_note_id, description, quantity,
                unit_price_minor, tax_rate_bps, tax_minor, line_total_minor
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(id)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(line.unit_price_minor)
        .bind(line.tax_rate_bps)
        .bind(totals.tax_minor)
        .bind(totals.line_total_minor)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let net = doc.total_minor - doc.tax_minor;
    let journal = credit_note_entry(id, currency, net, doc.tax_minor, doc.total_minor)
        .map_err(|e| validation(&request_id, format!("journal: {e}")))?;
    post_journal(&mut tx, org_id, &journal)
        .await
        .map_err(internal(&request_id))?;

    let new_credited = credited + doc.total_minor;
    let (new_balance, new_status) = balance_and_status(total, paid, new_credited, &status);
    let paid_at: Option<DateTime<Utc>> = if new_balance == 0 {
        Some(Utc::now())
    } else {
        None
    };

    sqlx::query(
        r#"
        UPDATE finance_invoice SET
            amount_credited_minor = $3,
            balance_minor = $4,
            status = $5,
            paid_at = COALESCE($6, paid_at),
            version = version + 1,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .bind(new_credited)
    .bind(new_balance)
    .bind(&new_status)
    .bind(paid_at)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "credit_note",
        "issued",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "invoice_id": inv_public,
            "credit_number": credit_number,
            "total_minor": doc.total_minor,
            "currency": currency_str,
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
        "finance.credit_note.create",
        "credit_note",
        &public_id.as_str(),
        serde_json::json!({ "invoice_id": inv_public }),
    )
    .await
    .map_err(internal(&request_id))?;

    let customer_public: String = sqlx::query_scalar(
        "SELECT public_id FROM finance_customer WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(customer_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let dto = CreditNoteDto {
        id: public_id.as_str(),
        invoice_id: inv_public,
        customer_id: customer_public,
        credit_number,
        currency: currency_str,
        subtotal_minor: doc.subtotal_minor - doc.discount_minor,
        tax_minor: doc.tax_minor,
        total_minor: doc.total_minor,
        reason: body.reason.clone(),
        issued_at: issued_at.to_rfc3339(),
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "credit_note.create",
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
