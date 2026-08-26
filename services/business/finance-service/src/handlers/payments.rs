//! `/api/v1/finance/payments` — record, allocate, list.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{
    balance_and_status, conflict, internal, normalize_paging, not_found, parse_public_id, validation,
};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::journal::{ensure_ledger_accounts, payment_entry, post_journal};
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    AllocatePaymentRequest, ListQuery, PaymentDto, PaymentListResponse, RecordPaymentRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/payments",
            get(list_payments).post(record_payment),
        )
        .route(
            "/api/v1/finance/payments/{id}/allocate",
            post(allocate_payment),
        )
}

#[derive(Debug, sqlx::FromRow)]
struct PaymentRow {
    public_id: String,
    customer_public_id: String,
    currency: String,
    amount_minor: i64,
    amount_allocated_minor: i64,
    amount_unapplied_minor: i64,
    method: String,
    provider: Option<String>,
    received_at: DateTime<Utc>,
    notes: Option<String>,
}

impl PaymentRow {
    fn into_dto(self) -> PaymentDto {
        PaymentDto {
            id: self.public_id,
            customer_id: self.customer_public_id,
            currency: self.currency,
            amount_minor: self.amount_minor,
            amount_allocated_minor: self.amount_allocated_minor,
            amount_unapplied_minor: self.amount_unapplied_minor,
            method: self.method,
            provider: self.provider,
            received_at: self.received_at.to_rfc3339(),
            notes: self.notes,
        }
    }
}

const PAYMENT_SELECT: &str = r#"
    p.public_id, c.public_id AS customer_public_id, p.currency, p.amount_minor,
    p.amount_allocated_minor, p.amount_unapplied_minor, p.method, p.provider,
    p.received_at, p.notes
"#;

async fn fetch_payment_dto(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    payment_id: Uuid,
) -> Result<Option<PaymentDto>, sqlx::Error> {
    let row: Option<PaymentRow> = sqlx::query_as(&format!(
        "SELECT {PAYMENT_SELECT}
         FROM finance_payment p
         JOIN finance_customer c ON c.id = p.customer_id
         WHERE p.org_id = $1 AND p.id = $2"
    ))
    .bind(org_id)
    .bind(payment_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(PaymentRow::into_dto))
}

/// Apply `amount` of a payment to an invoice; update invoice paid/balance/status.
pub(crate) async fn apply_allocation(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    payment_id: Uuid,
    invoice_id: Uuid,
    amount: i64,
    request_id: &str,
) -> Result<(), AppError> {
    if amount <= 0 {
        return Err(validation(request_id, "allocation amount must be positive"));
    }

    let inv: Option<(i64, i64, i64, i64, String)> = sqlx::query_as(
        r#"
        SELECT total_minor, amount_paid_minor, amount_credited_minor, balance_minor, status
        FROM finance_invoice
        WHERE org_id = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    let Some((total, paid, credited, balance, status)) = inv else {
        return Err(not_found(request_id, "invoice"));
    };
    if matches!(status.as_str(), "draft" | "void") {
        return Err(conflict(
            request_id,
            format!("cannot allocate to invoice in status {status}"),
        ));
    }
    if amount > balance {
        return Err(validation(
            request_id,
            format!("allocation {amount} exceeds invoice balance {balance}"),
        ));
    }

    let pay: Option<(i64,)> = sqlx::query_as(
        r#"
        SELECT amount_unapplied_minor FROM finance_payment
        WHERE org_id = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(org_id)
    .bind(payment_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    let Some((unapplied,)) = pay else {
        return Err(not_found(request_id, "payment"));
    };
    if amount > unapplied {
        return Err(validation(
            request_id,
            format!("allocation {amount} exceeds unapplied {unapplied}"),
        ));
    }

    sqlx::query(
        r#"
        INSERT INTO finance_payment_allocation (id, org_id, payment_id, invoice_id, amount_minor)
        VALUES ($1,$2,$3,$4,$5)
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id)
    .bind(payment_id)
    .bind(invoice_id)
    .bind(amount)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    sqlx::query(
        r#"
        UPDATE finance_payment SET
            amount_allocated_minor = amount_allocated_minor + $3,
            amount_unapplied_minor = amount_unapplied_minor - $3
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(payment_id)
    .bind(amount)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let new_paid = paid + amount;
    let (new_balance, new_status) = balance_and_status(total, new_paid, credited, &status);
    let paid_at: Option<DateTime<Utc>> = if new_balance == 0 {
        Some(Utc::now())
    } else {
        None
    };

    sqlx::query(
        r#"
        UPDATE finance_invoice SET
            amount_paid_minor = $3,
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
    .bind(new_paid)
    .bind(new_balance)
    .bind(&new_status)
    .bind(paid_at)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok(())
}

/// GET /api/v1/finance/payments
#[utoipa::path(get, path = "/api/v1/finance/payments", tag = "finance-payments",
    params(
        ("customer_id" = Option<String>, Query),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = PaymentListResponse)))]
pub async fn list_payments(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<PaymentListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_payment_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::finance_payment_read());
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM finance_payment WHERE org_id = ");
    count_qb.push_bind(org_id);
    push_owner_predicate(
        &mut count_qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    if let Some(ref cus) = q.customer_id {
        count_qb.push(
            " AND customer_id IN (SELECT id FROM finance_customer WHERE org_id = ",
        );
        count_qb.push_bind(org_id);
        count_qb.push(" AND public_id = ");
        count_qb.push_bind(cus.clone());
        count_qb.push(")");
    }
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {PAYMENT_SELECT}
         FROM finance_payment p
         JOIN finance_customer c ON c.id = p.customer_id
         WHERE p.org_id = "
    ));
    qb.push_bind(org_id);
    push_owner_predicate_on(
        &mut qb,
        "p.owner_user_id",
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    if let Some(ref cus) = q.customer_id {
        qb.push(" AND c.public_id = ");
        qb.push_bind(cus.clone());
    }
    qb.push(" ORDER BY p.received_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<PaymentRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(PaymentListResponse {
        items: rows.into_iter().map(PaymentRow::into_dto).collect(),
        total,
    }))
}

fn push_owner_predicate_on(
    qb: &mut QueryBuilder<'_, Postgres>,
    owner_col: &str,
    scope: companyos_authz::Scope,
    org_id: Uuid,
    actor_user_id: Uuid,
    team_id: Option<Uuid>,
    department_id: Option<Uuid>,
) {
    use companyos_authz::Scope;
    match scope {
        Scope::Organization => {}
        Scope::Own => {
            qb.push(format!(" AND {owner_col} = "));
            qb.push_bind(actor_user_id);
        }
        Scope::Team => {
            qb.push(format!(" AND ({owner_col} = "));
            qb.push_bind(actor_user_id);
            if let Some(team) = team_id {
                qb.push(format!(
                    " OR {owner_col} IN (SELECT user_id FROM membership WHERE org_id = "
                ));
                qb.push_bind(org_id);
                qb.push(" AND team_id = ");
                qb.push_bind(team);
                qb.push(" AND revoked_at IS NULL)");
            }
            qb.push(")");
        }
        Scope::Department => {
            qb.push(format!(" AND ({owner_col} = "));
            qb.push_bind(actor_user_id);
            if let Some(dept) = department_id {
                qb.push(format!(
                    " OR {owner_col} IN (SELECT user_id FROM membership WHERE org_id = "
                ));
                qb.push_bind(org_id);
                qb.push(" AND department_id = ");
                qb.push_bind(dept);
                qb.push(" AND revoked_at IS NULL)");
            }
            qb.push(")");
        }
    }
}

/// POST /api/v1/finance/payments
#[utoipa::path(post, path = "/api/v1/finance/payments", tag = "finance-payments",
    request_body = RecordPaymentRequest,
    responses((status = 201, body = PaymentDto)))]
pub async fn record_payment(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<RecordPaymentRequest>,
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
        perms::finance_payment_create(),
        &request_id,
    )?;

    if body.amount_minor <= 0 {
        return Err(validation(&request_id, "amount_minor must be positive"));
    }
    let currency = Currency::new(&body.currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let _: Uuid = parse_public_id(IdKind::Customer, &body.customer_id, &request_id)?;
    let invoice_uuid = match body.invoice_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::Invoice, s, &request_id)?),
        None => None,
    };
    let received_at = body
        .received_at
        .as_deref()
        .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
        .transpose()
        .map_err(|_| validation(&request_id, "received_at must be RFC3339"))?
        .unwrap_or_else(Utc::now);

    let public_id = PublicId::generate(IdKind::Payment);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "payment.record", key)
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

    // Determine allocation vs overpayment (unapplied / customer credits).
    let mut allocate_amount = 0i64;
    if let Some(inv_id) = invoice_uuid {
        let bal: Option<(i64, String, Uuid)> = sqlx::query_as(
            r#"
            SELECT balance_minor, status, customer_id FROM finance_invoice
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(inv_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        let Some((balance, status, inv_customer)) = bal else {
            return Err(not_found(&request_id, "invoice"));
        };
        if inv_customer != customer_id {
            return Err(validation(
                &request_id,
                "invoice does not belong to customer",
            ));
        }
        if matches!(status.as_str(), "draft" | "void") {
            return Err(conflict(
                &request_id,
                format!("cannot pay invoice in status {status}"),
            ));
        }
        allocate_amount = body.amount_minor.min(balance);
    }
    let unapplied = body.amount_minor - allocate_amount;

    sqlx::query(
        r#"
        INSERT INTO finance_payment (
            id, org_id, public_id, customer_id, owner_user_id, currency,
            amount_minor, amount_allocated_minor, amount_unapplied_minor,
            method, received_at, notes
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,0,$7,'manual',$8,$9)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(customer_id)
    .bind(auth.ctx.actor.user_id)
    .bind(&body.currency)
    .bind(body.amount_minor)
    .bind(received_at)
    .bind(&body.notes)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    // Journal: full cash in; AR for allocated; customer credits for unapplied.
    let journal = payment_entry(id, currency, allocate_amount, unapplied)
        .map_err(|e| validation(&request_id, format!("journal: {e}")))?;
    post_journal(&mut tx, org_id, &journal)
        .await
        .map_err(internal(&request_id))?;

    if let Some(inv_id) = invoice_uuid {
        if allocate_amount > 0 {
            apply_allocation(&mut tx, org_id, id, inv_id, allocate_amount, &request_id).await?;

            let inv_public = PublicId::new(IdKind::Invoice, inv_id).as_str();
            let paid_env = EventEnvelope::new(
                auth.ctx.org_id,
                Context::Finance,
                "invoice",
                "paid",
                1,
                auth.ctx.actor.clone(),
                serde_json::json!({
                    "id": inv_public,
                    "payment_id": public_id.as_str(),
                    "amount_minor": allocate_amount,
                }),
            );
            companyos_outbox::insert_event(&mut *tx, &paid_env)
                .await
                .map_err(|e| {
                    AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
                })?;

            let alloc_env = EventEnvelope::new(
                auth.ctx.org_id,
                Context::Finance,
                "payment",
                "allocated",
                1,
                auth.ctx.actor.clone(),
                serde_json::json!({
                    "payment_id": public_id.as_str(),
                    "invoice_id": inv_public,
                    "amount_minor": allocate_amount,
                }),
            );
            companyos_outbox::insert_event(&mut *tx, &alloc_env)
                .await
                .map_err(|e| {
                    AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
                })?;
        }
    } else {
        // Unallocated payment still emits PaymentAllocated? Spec says InvoicePaid + PaymentAllocated
        // on pay — only when allocated. Record a payment.received event.
        let env = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Finance,
            "payment",
            "received",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "id": public_id.as_str(),
                "amount_minor": body.amount_minor,
                "unapplied_minor": unapplied,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &env)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }

    // If overpayment with invoice, also emit received for unapplied portion via payment.received.
    if invoice_uuid.is_some() && unapplied > 0 {
        let env = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Finance,
            "payment",
            "received",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "id": public_id.as_str(),
                "amount_minor": body.amount_minor,
                "unapplied_minor": unapplied,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &env)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.payment.record",
        "payment",
        &public_id.as_str(),
        serde_json::json!({
            "amount_minor": body.amount_minor,
            "allocated_minor": allocate_amount,
            "unapplied_minor": unapplied,
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_payment_dto(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "payment missing after insert",
            )
        })?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "payment.record",
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

/// POST /api/v1/finance/payments/{id}/allocate
#[utoipa::path(post, path = "/api/v1/finance/payments/{id}/allocate", tag = "finance-payments",
    request_body = AllocatePaymentRequest,
    responses((status = 200, body = PaymentDto)))]
pub async fn allocate_payment(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AllocatePaymentRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let payment_id = parse_public_id(IdKind::Payment, &id, &request_id)?;
    let invoice_id = parse_public_id(IdKind::Invoice, &body.invoice_id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);
    let idem_scope = format!("payment.allocate.{id}");

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_payment_allocate(),
        &request_id,
    )?;

    if body.amount_minor <= 0 {
        return Err(validation(&request_id, "amount_minor must be positive"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, &idem_scope, key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK);
            return Ok((code, Json(stored)).into_response());
        }
    }

    // Journal for late allocation: move from customer credits to AR.
    // When payment was fully unapplied at record time, cash+credits already posted.
    // Allocating later: Dr Customer Credits, Cr AR.
    let currency_str: String = sqlx::query_scalar(
        "SELECT currency FROM finance_payment WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(payment_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .ok_or_else(|| not_found(&request_id, "payment"))?;
    let currency = Currency::new(&currency_str)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;

    apply_allocation(
        &mut tx,
        org_id,
        payment_id,
        invoice_id,
        body.amount_minor,
        &request_id,
    )
    .await?;

    // Reclassify liability → AR settlement for previously unapplied cash.
    let journal = crate::journal::JournalDraft {
        memo: format!("Allocate payment {payment_id}"),
        source_type: "payment",
        source_id: payment_id,
        currency,
        lines: vec![
            crate::journal::LedgerLine {
                account_code: crate::journal::codes::CUSTOMER_CREDITS,
                debit_minor: body.amount_minor,
                credit_minor: 0,
                memo: Some("Apply customer credit".into()),
            },
            crate::journal::LedgerLine {
                account_code: crate::journal::codes::AR,
                debit_minor: 0,
                credit_minor: body.amount_minor,
                memo: Some("AR settlement from credit".into()),
            },
        ],
    };
    journal
        .assert_balanced()
        .map_err(|e| validation(&request_id, format!("journal: {e}")))?;
    post_journal(&mut tx, org_id, &journal)
        .await
        .map_err(internal(&request_id))?;

    let inv_public = PublicId::new(IdKind::Invoice, invoice_id).as_str();
    let pay_public = PublicId::new(IdKind::Payment, payment_id).as_str();

    let paid_env = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "invoice",
        "paid",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": inv_public,
            "payment_id": pay_public,
            "amount_minor": body.amount_minor,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &paid_env)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let alloc_env = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "payment",
        "allocated",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "payment_id": pay_public,
            "invoice_id": inv_public,
            "amount_minor": body.amount_minor,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &alloc_env)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.payment.allocate",
        "payment",
        &pay_public,
        serde_json::json!({
            "invoice_id": inv_public,
            "amount_minor": body.amount_minor,
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_payment_dto(&mut tx, org_id, payment_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "payment"))?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            &idem_scope,
            key,
            200,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto).into_response())
}
