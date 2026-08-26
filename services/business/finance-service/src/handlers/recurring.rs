//! `/api/v1/finance/recurring` — templates + simple run-due scheduler hook.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Months, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::invoice_math::{compute_document_totals, LineInput};
use crate::journal::ensure_ledger_accounts;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreateInvoiceRequest, CreateRecurringRequest, RecurringInvoiceDto, RunDueResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/finance/recurring", post(create_recurring))
        .route("/api/v1/finance/recurring/run-due", post(run_due))
}

fn advance_next_run(from: DateTime<Utc>, cadence: &str) -> Result<DateTime<Utc>, AppError> {
    match cadence {
        "monthly" => Ok(from + Months::new(1)),
        "quarterly" => Ok(from + Months::new(3)),
        "yearly" => Ok(from + Months::new(12)),
        _ => Err(validation(
            "unknown",
            format!("unsupported cadence: {cadence}"),
        )),
    }
}

/// POST /api/v1/finance/recurring
#[utoipa::path(post, path = "/api/v1/finance/recurring", tag = "finance-recurring",
    request_body = CreateRecurringRequest,
    responses((status = 201, body = RecurringInvoiceDto)))]
pub async fn create_recurring(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateRecurringRequest>,
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
        perms::finance_invoice_create(),
        &request_id,
    )?;

    if !matches!(body.cadence.as_str(), "monthly" | "quarterly" | "yearly") {
        return Err(validation(
            &request_id,
            "cadence must be monthly|quarterly|yearly",
        ));
    }
    let _: Uuid = parse_public_id(IdKind::Customer, &body.customer_id, &request_id)?;
    let next_run_at = DateTime::parse_from_rfc3339(&body.next_run_at)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| validation(&request_id, "next_run_at must be RFC3339"))?;

    let template = serde_json::to_value(&body.template)
        .map_err(|e| validation(&request_id, format!("template json: {e}")))?;

    let id = new_uuid_v7();
    let public_id = format!("rci_{id}");

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "recurring.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let customer_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM finance_customer WHERE org_id = $1 AND public_id = $2",
    )
    .bind(org_id)
    .bind(&body.customer_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .ok_or_else(|| not_found(&request_id, "customer"))?;

    sqlx::query(
        r#"
        INSERT INTO finance_recurring_invoice (
            id, org_id, public_id, customer_id, owner_user_id,
            cadence, next_run_at, active, template
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,true,$8)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(&public_id)
    .bind(customer_id)
    .bind(auth.ctx.actor.user_id)
    .bind(&body.cadence)
    .bind(next_run_at)
    .bind(&template)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "recurring_invoice",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": public_id, "cadence": body.cadence }),
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
        "finance.recurring.create",
        "recurring_invoice",
        &public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = RecurringInvoiceDto {
        id: public_id,
        customer_id: body.customer_id.clone(),
        cadence: body.cadence.clone(),
        next_run_at: next_run_at.to_rfc3339(),
        active: true,
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "recurring.create",
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

#[derive(sqlx::FromRow)]
struct DueRow {
    id: Uuid,
    public_id: String,
    customer_id: Uuid,
    owner_user_id: Uuid,
    cadence: String,
    next_run_at: DateTime<Utc>,
    template: serde_json::Value,
}

/// POST /api/v1/finance/recurring/run-due — create draft invoices from due templates.
#[utoipa::path(post, path = "/api/v1/finance/recurring/run-due", tag = "finance-recurring",
    responses((status = 200, body = RunDueResponse)))]
pub async fn run_due(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<RunDueResponse>, AppError> {
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
        perms::finance_invoice_create(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let due: Vec<DueRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, customer_id, owner_user_id, cadence, next_run_at, template
        FROM finance_recurring_invoice
        WHERE org_id = $1 AND active = true AND next_run_at <= now()
        ORDER BY next_run_at ASC
        FOR UPDATE
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut created = Vec::new();
    for row in due {
        let template: CreateInvoiceRequest = serde_json::from_value(row.template.clone())
            .map_err(|e| validation(&request_id, format!("bad recurring template: {e}")))?;
        let currency = Currency::new(&template.currency)
            .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
        if template.lines.is_empty() {
            continue;
        }
        let inputs: Vec<LineInput> = template
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
            .map_err(|e| validation(&request_id, format!("invalid totals: {e}")))?;

        let inv_public = PublicId::generate(IdKind::Invoice);
        let inv_id = inv_public.uuid();

        sqlx::query(
            r#"
            INSERT INTO finance_invoice (
                id, org_id, public_id, customer_id, owner_user_id, status,
                currency, base_currency, subtotal_minor, discount_minor, tax_minor,
                total_minor, base_total_minor, balance_minor, notes, terms
            ) VALUES ($1,$2,$3,$4,$5,'draft',$6,$7,$8,$9,$10,$11,0,0,$12,$13)
            "#,
        )
        .bind(inv_id)
        .bind(org_id)
        .bind(inv_public.as_str())
        .bind(row.customer_id)
        .bind(row.owner_user_id)
        .bind(&template.currency)
        .bind(&template.base_currency)
        .bind(doc.subtotal_minor)
        .bind(doc.discount_minor)
        .bind(doc.tax_minor)
        .bind(doc.total_minor)
        .bind(&template.notes)
        .bind(&template.terms)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        for (position, (line, totals)) in template.lines.iter().zip(computed.iter()).enumerate() {
            let line_id = new_uuid_v7();
            let line_public = PublicId::new(IdKind::Invoice, line_id)
                .as_str()
                .replacen("inv_", "inl_", 1);
            sqlx::query(
                r#"
                INSERT INTO finance_invoice_line (
                    id, org_id, invoice_id, public_id, position, description,
                    quantity, unit_price_minor, discount_minor, tax_rate_bps,
                    tax_minor, line_total_minor
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
                "#,
            )
            .bind(line_id)
            .bind(org_id)
            .bind(inv_id)
            .bind(&line_public)
            .bind(position as i32)
            .bind(&line.description)
            .bind(line.quantity)
            .bind(line.unit_price_minor)
            .bind(line.discount_minor)
            .bind(line.tax_rate_bps)
            .bind(totals.tax_minor)
            .bind(totals.line_total_minor)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        }

        let next = advance_next_run(row.next_run_at, &row.cadence)
            .map_err(|e| validation(&request_id, e.detail))?;
        // Ensure next is in the future even if we were far behind.
        let mut next = next;
        let now = Utc::now();
        while next <= now {
            next = advance_next_run(next, &row.cadence)
                .map_err(|e| validation(&request_id, e.detail))?;
        }

        sqlx::query(
            r#"
            UPDATE finance_recurring_invoice SET
                next_run_at = $3, last_invoice_id = $4
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(row.id)
        .bind(next)
        .bind(inv_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        let envelope = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Finance,
            "invoice",
            "created",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "id": inv_public.as_str(),
                "from_recurring": row.public_id,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &envelope)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

        created.push(inv_public.as_str());
        let _ = row.next_run_at.year(); // keep Datelike import used if needed
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.recurring.run_due",
        "recurring_invoice",
        "run-due",
        serde_json::json!({ "created": created.len() }),
    )
    .await
    .map_err(internal(&request_id))?;

    let processed = created.len() as i64;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(RunDueResponse {
        created_invoice_ids: created,
        processed,
    }))
}
