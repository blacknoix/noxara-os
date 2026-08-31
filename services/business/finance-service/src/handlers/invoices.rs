//! `/api/v1/finance/invoices` — draft CRUD, issue, send, void, from-quote.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{
    conflict, if_match_version, internal, normalize_paging, not_found, parse_optional_public_id,
    parse_public_id, validation,
};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::handlers::entities::resolve_entity_id;
use crate::handlers::tax::resolve_rate_bps;
use crate::idempotency;
use crate::invoice_math::{compute_document_totals, convert_to_base, LineInput};
use crate::journal::{ensure_ledger_accounts, invoice_issue_entry, post_journal};
use crate::numbering::next_invoice_number;
use crate::principal::{
    enforce_any_scope, enforce_scoped, load_membership_scope_for, required_scope_for_owner_row,
    MembershipScope,
};
use crate::projection::ensure_customer_from_snapshot;
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreateInvoiceFromQuoteRequest, CreateInvoiceRequest, InvoiceDto, InvoiceLineDto,
    InvoiceLineInput, InvoiceListResponse, IssueInvoiceRequest, ListQuery, UpdateInvoiceRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/invoices",
            get(list_invoices).post(create_invoice),
        )
        .route(
            "/api/v1/finance/invoices/from-quote",
            post(create_from_quote),
        )
        .route(
            "/api/v1/finance/invoices/{id}",
            get(get_invoice).patch(update_invoice),
        )
        .route("/api/v1/finance/invoices/{id}/issue", post(issue_invoice))
        .route("/api/v1/finance/invoices/{id}/send", post(send_invoice))
        .route("/api/v1/finance/invoices/{id}/void", post(void_invoice))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct InvoiceRow {
    id: Uuid,
    public_id: String,
    #[allow(dead_code)]
    customer_id: Uuid,
    customer_public_id: String,
    owner_user_id: Uuid,
    status: String,
    invoice_number: Option<String>,
    currency: String,
    base_currency: String,
    fx_rate_num: Option<i64>,
    fx_rate_den: Option<i64>,
    fx_rate_date: Option<NaiveDate>,
    subtotal_minor: i64,
    discount_minor: i64,
    tax_minor: i64,
    total_minor: i64,
    base_total_minor: i64,
    amount_paid_minor: i64,
    amount_credited_minor: i64,
    balance_minor: i64,
    issue_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    payment_url: Option<String>,
    source_quote_public_id: Option<String>,
    notes: Option<String>,
    terms: Option<String>,
    entity_public_id: Option<String>,
    #[allow(dead_code)]
    entity_id: Option<Uuid>,
    version: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct LineRow {
    invoice_id: Uuid,
    public_id: String,
    description: String,
    quantity: i64,
    unit_price_minor: i64,
    discount_minor: i64,
    tax_rate_bps: i64,
    tax_minor: i64,
    line_total_minor: i64,
    tax_rate_public_id: Option<String>,
    tax_group_public_id: Option<String>,
}

const INVOICE_SELECT: &str = r#"
    i.id, i.public_id, i.customer_id, c.public_id AS customer_public_id, i.owner_user_id,
    i.status, i.invoice_number, i.currency, i.base_currency, i.fx_rate_num, i.fx_rate_den,
    i.fx_rate_date, i.subtotal_minor, i.discount_minor, i.tax_minor, i.total_minor,
    i.base_total_minor, i.amount_paid_minor, i.amount_credited_minor, i.balance_minor,
    i.issue_date, i.due_date, i.payment_url, i.source_quote_public_id, i.notes, i.terms,
    e.public_id AS entity_public_id, i.entity_id,
    i.version, i.created_at, i.updated_at
"#;

fn assemble_dto(row: InvoiceRow, lines: Vec<LineRow>) -> InvoiceDto {
    InvoiceDto {
        id: row.public_id,
        customer_id: row.customer_public_id,
        status: row.status,
        invoice_number: row.invoice_number,
        currency: row.currency,
        base_currency: row.base_currency,
        fx_rate_num: row.fx_rate_num,
        fx_rate_den: row.fx_rate_den,
        fx_rate_date: row.fx_rate_date.map(|d| d.to_string()),
        subtotal_minor: row.subtotal_minor,
        discount_minor: row.discount_minor,
        tax_minor: row.tax_minor,
        total_minor: row.total_minor,
        base_total_minor: row.base_total_minor,
        amount_paid_minor: row.amount_paid_minor,
        amount_credited_minor: row.amount_credited_minor,
        balance_minor: row.balance_minor,
        issue_date: row.issue_date.map(|d| d.to_string()),
        due_date: row.due_date.map(|d| d.to_string()),
        payment_url: row.payment_url,
        source_quote_id: row.source_quote_public_id,
        notes: row.notes,
        terms: row.terms,
        entity_id: row.entity_public_id,
        version: row.version,
        lines: lines
            .into_iter()
            .map(|l| InvoiceLineDto {
                id: l.public_id,
                description: l.description,
                quantity: l.quantity,
                unit_price_minor: l.unit_price_minor,
                discount_minor: l.discount_minor,
                tax_rate_bps: l.tax_rate_bps,
                tax_minor: l.tax_minor,
                line_total_minor: l.line_total_minor,
                tax_rate_id: l.tax_rate_public_id,
                tax_group_id: l.tax_group_public_id,
            })
            .collect(),
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    }
}

async fn fetch_lines(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<LineRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT l.invoice_id, l.public_id, l.description, l.quantity, l.unit_price_minor,
               l.discount_minor, l.tax_rate_bps, l.tax_minor, l.line_total_minor,
               tr.public_id AS tax_rate_public_id, tg.public_id AS tax_group_public_id
        FROM finance_invoice_line l
        LEFT JOIN finance_tax_rate tr ON tr.id = l.tax_rate_id
        LEFT JOIN finance_tax_group tg ON tg.id = l.tax_group_id
        WHERE l.org_id = $1 AND l.invoice_id = $2
        ORDER BY l.position ASC
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .fetch_all(&mut **tx)
    .await
}

/// Batch-load lines for many invoices in one query (avoids N+1 in list_invoices).
/// Before: 1 invoices query + N fetch_lines. After: 1 invoices + 1 batch lines.
async fn fetch_lines_batch(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    invoice_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<LineRow>>, sqlx::Error> {
    use std::collections::HashMap;
    let mut map: HashMap<Uuid, Vec<LineRow>> = HashMap::new();
    if invoice_ids.is_empty() {
        return Ok(map);
    }
    let rows: Vec<LineRow> = sqlx::query_as(
        r#"
        SELECT l.invoice_id, l.public_id, l.description, l.quantity, l.unit_price_minor,
               l.discount_minor, l.tax_rate_bps, l.tax_minor, l.line_total_minor,
               tr.public_id AS tax_rate_public_id, tg.public_id AS tax_group_public_id
        FROM finance_invoice_line l
        LEFT JOIN finance_tax_rate tr ON tr.id = l.tax_rate_id
        LEFT JOIN finance_tax_group tg ON tg.id = l.tax_group_id
        WHERE l.org_id = $1 AND l.invoice_id = ANY($2)
        ORDER BY l.invoice_id, l.position ASC
        "#,
    )
    .bind(org_id)
    .bind(invoice_ids)
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        map.entry(row.invoice_id).or_default().push(row);
    }
    Ok(map)
}

async fn fetch_invoice_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    invoice_id: Uuid,
) -> Result<Option<InvoiceRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {INVOICE_SELECT} FROM finance_invoice i
         JOIN finance_customer c ON c.id = i.customer_id
         LEFT JOIN finance_entity e ON e.id = i.entity_id
         WHERE i.org_id = $1 AND i.id = $2"
    ))
    .bind(org_id)
    .bind(invoice_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn fetch_invoice_dto(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    invoice_id: Uuid,
) -> Result<Option<InvoiceDto>, sqlx::Error> {
    let Some(row) = fetch_invoice_row(tx, org_id, invoice_id).await? else {
        return Ok(None);
    };
    let lines = fetch_lines(tx, org_id, row.id).await?;
    Ok(Some(assemble_dto(row, lines)))
}

fn line_inputs(lines: &[InvoiceLineInput]) -> Vec<LineInput> {
    lines
        .iter()
        .map(|l| LineInput {
            quantity: l.quantity,
            unit_price_minor: l.unit_price_minor,
            discount_minor: l.discount_minor,
            tax_rate_bps: l.tax_rate_bps,
        })
        .collect()
}

async fn insert_lines(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    invoice_id: Uuid,
    lines: &[InvoiceLineInput],
    computed: &[crate::invoice_math::LineTotals],
    request_id: &str,
) -> Result<(), AppError> {
    for (position, (line, totals)) in lines.iter().zip(computed.iter()).enumerate() {
        let line_id = new_uuid_v7();
        let line_public = PublicId::new(IdKind::Invoice, line_id)
            .as_str()
            .replacen("inv_", "inl_", 1);
        let tax_rate_uuid =
            parse_optional_public_id(IdKind::TaxRate, line.tax_rate_id.as_deref(), request_id)?;
        let tax_group_uuid =
            parse_optional_public_id(IdKind::TaxGroup, line.tax_group_id.as_deref(), request_id)?;
        sqlx::query(
            r#"
            INSERT INTO finance_invoice_line (
                id, org_id, invoice_id, public_id, position, description,
                quantity, unit_price_minor, discount_minor, tax_rate_bps,
                tax_minor, line_total_minor, tax_rate_id, tax_group_id
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
            "#,
        )
        .bind(line_id)
        .bind(org_id)
        .bind(invoice_id)
        .bind(&line_public)
        .bind(position as i32)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(line.unit_price_minor)
        .bind(line.discount_minor)
        .bind(line.tax_rate_bps)
        .bind(totals.tax_minor)
        .bind(totals.line_total_minor)
        .bind(tax_rate_uuid)
        .bind(tax_group_uuid)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }
    Ok(())
}

async fn resolve_customer_id(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    customer_public_id: &str,
    request_id: &str,
) -> Result<Uuid, AppError> {
    let _: Uuid = parse_public_id(IdKind::Customer, customer_public_id, request_id)?;
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM finance_customer WHERE org_id = $1 AND public_id = $2")
            .bind(org_id)
            .bind(customer_public_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal(request_id))?;
    row.map(|r| r.0)
        .ok_or_else(|| not_found(request_id, "customer"))
}

async fn enforce_invoice_scope(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    auth: &AuthCtx,
    membership: &MembershipScope,
    permission: companyos_authz::PermissionId,
    owner_user_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    let required_scope = required_scope_for_owner_row(
        tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        Some(owner_user_id),
    )
    .await
    .map_err(internal(request_id))?;
    enforce_scoped(
        &membership.principal,
        permission,
        required_scope,
        request_id,
    )
}

fn parse_date(
    raw: Option<&str>,
    field: &str,
    request_id: &str,
) -> Result<Option<NaiveDate>, AppError> {
    raw.map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| validation(request_id, format!("{field} must be YYYY-MM-DD")))
}

/// At issue: resolve tax_group_id / tax_rate_id as of issue_date into tax_rate_bps
/// snapshots, recompute line + document totals. Returns (tax_minor, total_minor).
async fn snapshot_taxes_at_issue(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    invoice_id: Uuid,
    issue_date: NaiveDate,
    row: &InvoiceRow,
    request_id: &str,
) -> Result<(i64, i64), AppError> {
    #[derive(sqlx::FromRow)]
    struct RawLine {
        id: Uuid,
        quantity: i64,
        unit_price_minor: i64,
        discount_minor: i64,
        tax_rate_bps: i64,
        tax_rate_id: Option<Uuid>,
        tax_group_id: Option<Uuid>,
    }

    let raw_lines: Vec<RawLine> = sqlx::query_as(
        r#"
        SELECT id, quantity, unit_price_minor, discount_minor, tax_rate_bps,
               tax_rate_id, tax_group_id
        FROM finance_invoice_line
        WHERE org_id = $1 AND invoice_id = $2
        ORDER BY position ASC
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let mut inputs = Vec::with_capacity(raw_lines.len());
    for line in &raw_lines {
        let mut rate_bps = line.tax_rate_bps;
        let mut resolved_rate_id = line.tax_rate_id;
        if line.tax_group_id.is_some() || line.tax_rate_id.is_some() {
            if let Some((rid, bps, _)) =
                resolve_rate_bps(tx, org_id, line.tax_group_id, line.tax_rate_id, issue_date)
                    .await
                    .map_err(internal(request_id))?
            {
                rate_bps = bps;
                resolved_rate_id = Some(rid);
            }
        }
        inputs.push((
            line.id,
            resolved_rate_id,
            LineInput {
                quantity: line.quantity,
                unit_price_minor: line.unit_price_minor,
                discount_minor: line.discount_minor,
                tax_rate_bps: rate_bps,
            },
        ));
    }

    let line_inputs_only: Vec<LineInput> = inputs.iter().map(|(_, _, i)| *i).collect();
    let currency = Currency::new(&row.currency)
        .map_err(|e| validation(request_id, format!("invalid currency: {e}")))?;
    let (computed, doc) = compute_document_totals(&line_inputs_only, currency)
        .map_err(|e| validation(request_id, format!("invalid line totals: {e}")))?;

    for ((line_id, rate_id, input), totals) in inputs.iter().zip(computed.iter()) {
        sqlx::query(
            r#"
            UPDATE finance_invoice_line SET
                tax_rate_bps = $3, tax_rate_id = COALESCE($4, tax_rate_id),
                tax_minor = $5, line_total_minor = $6
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(line_id)
        .bind(input.tax_rate_bps)
        .bind(rate_id)
        .bind(totals.tax_minor)
        .bind(totals.line_total_minor)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }

    sqlx::query(
        r#"
        UPDATE finance_invoice SET
            subtotal_minor = $3, discount_minor = $4, tax_minor = $5, total_minor = $6
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .bind(doc.subtotal_minor)
    .bind(doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok((doc.tax_minor, doc.total_minor))
}

/// GET /api/v1/finance/invoices
#[utoipa::path(get, path = "/api/v1/finance/invoices", tag = "finance-invoices",
    params(
        ("q" = Option<String>, Query),
        ("status" = Option<String>, Query),
        ("customer_id" = Option<String>, Query),
        ("entity_id" = Option<String>, Query),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = InvoiceListResponse)))]
pub async fn list_invoices(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<InvoiceListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let org_public = auth.ctx.org_id.to_public().as_str().to_string();
    let _timer = companyos_telemetry::RedTimer::start(format!("{org_public}:list_invoices"));
    let actor = auth.ctx.actor.user_id;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::finance_invoice_read());
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    if let Some(cus) = q.customer_id.as_deref() {
        let _ = parse_public_id(IdKind::Customer, cus, &request_id)?;
    }
    if let Some(ent) = q.entity_id.as_deref() {
        let _ = parse_public_id(IdKind::FinanceEntity, ent, &request_id)?;
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM finance_invoice i WHERE i.org_id = ");
    count_qb.push_bind(org_id);
    push_owner_predicate(
        &mut count_qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    if let Some(status) = q.status.as_deref() {
        count_qb.push(" AND i.status = ");
        count_qb.push_bind(status);
    }
    if let Some(ref cus) = q.customer_id {
        count_qb.push(" AND i.customer_id IN (SELECT id FROM finance_customer WHERE org_id = ");
        count_qb.push_bind(org_id);
        count_qb.push(" AND public_id = ");
        count_qb.push_bind(cus.clone());
        count_qb.push(")");
    }
    if let Some(ref ent) = q.entity_id {
        count_qb.push(" AND i.entity_id IN (SELECT id FROM finance_entity WHERE org_id = ");
        count_qb.push_bind(org_id);
        count_qb.push(" AND public_id = ");
        count_qb.push_bind(ent.clone());
        count_qb.push(")");
    }
    if let Some(qtext) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = format!("%{qtext}%");
        count_qb.push(" AND (i.invoice_number ILIKE ");
        count_qb.push_bind(pattern.clone());
        count_qb.push(" OR i.notes ILIKE ");
        count_qb.push_bind(pattern);
        count_qb.push(")");
    }
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {INVOICE_SELECT} FROM finance_invoice i
         JOIN finance_customer c ON c.id = i.customer_id
         LEFT JOIN finance_entity e ON e.id = i.entity_id
         WHERE i.org_id = "
    ));
    qb.push_bind(org_id);
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    if let Some(status) = q.status.as_deref() {
        qb.push(" AND i.status = ");
        qb.push_bind(status);
    }
    if let Some(ref cus) = q.customer_id {
        qb.push(" AND c.public_id = ");
        qb.push_bind(cus.clone());
    }
    if let Some(ref ent) = q.entity_id {
        qb.push(" AND e.public_id = ");
        qb.push_bind(ent.clone());
    }
    if let Some(qtext) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = format!("%{qtext}%");
        qb.push(" AND (i.invoice_number ILIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR i.notes ILIKE ");
        qb.push_bind(pattern);
        qb.push(")");
    }
    qb.push(" ORDER BY i.created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<InvoiceRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let invoice_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut lines_by_invoice = fetch_lines_batch(&mut tx, org_id, &invoice_ids)
        .await
        .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let lines = lines_by_invoice.remove(&row.id).unwrap_or_default();
        items.push(assemble_dto(row, lines));
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(InvoiceListResponse { items, total }))
}

/// POST /api/v1/finance/invoices
#[utoipa::path(post, path = "/api/v1/finance/invoices", tag = "finance-invoices",
    request_body = CreateInvoiceRequest,
    responses((status = 201, body = InvoiceDto)))]
pub async fn create_invoice(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateInvoiceRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_create(),
        &request_id,
    )?;

    if body.lines.is_empty() {
        return Err(validation(
            &request_id,
            "invoice requires at least one line",
        ));
    }
    let currency = Currency::new(&body.currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let _base = Currency::new(&body.base_currency)
        .map_err(|e| validation(&request_id, format!("invalid base_currency: {e}")))?;
    let due_date = parse_date(body.due_date.as_deref(), "due_date", &request_id)?;
    let inputs = line_inputs(&body.lines);
    let (computed, doc) = compute_document_totals(&inputs, currency)
        .map_err(|e| validation(&request_id, format!("invalid line totals: {e}")))?;

    let public_id = PublicId::generate(IdKind::Invoice);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "invoice.create", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let customer_id = resolve_customer_id(&mut tx, org_id, &body.customer_id, &request_id).await?;
    let (entity_uuid, _) =
        resolve_entity_id(&mut tx, org_id, body.entity_id.as_deref(), &request_id).await?;

    sqlx::query(
        r#"
        INSERT INTO finance_invoice (
            id, org_id, public_id, customer_id, owner_user_id, status,
            currency, base_currency, subtotal_minor, discount_minor, tax_minor,
            total_minor, base_total_minor, balance_minor, due_date, notes, terms, entity_id
        ) VALUES ($1,$2,$3,$4,$5,'draft',$6,$7,$8,$9,$10,$11,0,0,$12,$13,$14,$15)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(customer_id)
    .bind(auth.ctx.actor.user_id)
    .bind(&body.currency)
    .bind(&body.base_currency)
    .bind(doc.subtotal_minor)
    .bind(doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .bind(due_date)
    .bind(&body.notes)
    .bind(&body.terms)
    .bind(entity_uuid)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_lines(&mut tx, org_id, id, &body.lines, &computed, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "invoice",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "status": "draft",
            "total_minor": doc.total_minor,
            "currency": body.currency,
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
        "finance.invoice.create",
        "invoice",
        &public_id.as_str(),
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_invoice_dto(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "invoice missing after insert",
            )
        })?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "invoice.create",
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

/// GET /api/v1/finance/invoices/{id}
#[utoipa::path(get, path = "/api/v1/finance/invoices/{id}", tag = "finance-invoices",
    responses((status = 200, body = InvoiceDto), (status = 404)))]
pub async fn get_invoice(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<InvoiceDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let invoice_id = parse_public_id(IdKind::Invoice, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_invoice_row(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;

    enforce_invoice_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::finance_invoice_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    let lines = fetch_lines(&mut tx, org_id, row.id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(assemble_dto(row, lines)))
}

/// PATCH /api/v1/finance/invoices/{id} — draft only; requires If-Match version.
#[utoipa::path(patch, path = "/api/v1/finance/invoices/{id}", tag = "finance-invoices",
    request_body = UpdateInvoiceRequest,
    responses((status = 200, body = InvoiceDto)))]
pub async fn update_invoice(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateInvoiceRequest>,
) -> Result<Json<InvoiceDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let invoice_id = parse_public_id(IdKind::Invoice, &id, &request_id)?;
    let expected = if_match_version(&headers)
        .ok_or_else(|| validation(&request_id, "If-Match version required"))?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let row = fetch_invoice_row(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;

    enforce_invoice_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::finance_invoice_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status != "draft" {
        return Err(conflict(&request_id, "only draft invoices can be updated"));
    }
    if row.version != expected {
        return Err(conflict(
            &request_id,
            format!("version mismatch: expected {expected}, got {}", row.version),
        ));
    }

    let due_date = if body.due_date.is_some() {
        parse_date(body.due_date.as_deref(), "due_date", &request_id)?
    } else {
        row.due_date
    };
    let notes = body.notes.clone().or(row.notes.clone());
    let terms = body.terms.clone().or(row.terms.clone());

    let (subtotal, discount, tax, total) = if let Some(ref lines) = body.lines {
        if lines.is_empty() {
            return Err(validation(
                &request_id,
                "invoice requires at least one line",
            ));
        }
        let currency = Currency::new(&row.currency)
            .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
        let inputs = line_inputs(lines);
        let (computed, doc) = compute_document_totals(&inputs, currency)
            .map_err(|e| validation(&request_id, format!("invalid line totals: {e}")))?;

        sqlx::query("DELETE FROM finance_invoice_line WHERE org_id = $1 AND invoice_id = $2")
            .bind(org_id)
            .bind(invoice_id)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        insert_lines(&mut tx, org_id, invoice_id, lines, &computed, &request_id).await?;
        (
            doc.subtotal_minor,
            doc.discount_minor,
            doc.tax_minor,
            doc.total_minor,
        )
    } else {
        (
            row.subtotal_minor,
            row.discount_minor,
            row.tax_minor,
            row.total_minor,
        )
    };

    sqlx::query(
        r#"
        UPDATE finance_invoice SET
            subtotal_minor = $3, discount_minor = $4, tax_minor = $5, total_minor = $6,
            due_date = $7, notes = $8, terms = $9,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND status = 'draft' AND version = $10
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .bind(subtotal)
    .bind(discount)
    .bind(tax)
    .bind(total)
    .bind(due_date)
    .bind(&notes)
    .bind(&terms)
    .bind(expected)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "invoice",
        "updated",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": row.public_id, "version": expected + 1 }),
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
        "finance.invoice.update",
        "invoice",
        &row.public_id,
        serde_json::json!({ "version": expected + 1 }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_invoice_dto(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/finance/invoices/{id}/issue
#[utoipa::path(post, path = "/api/v1/finance/invoices/{id}/issue", tag = "finance-invoices",
    request_body = IssueInvoiceRequest,
    responses((status = 200, body = InvoiceDto)))]
pub async fn issue_invoice(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<IssueInvoiceRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let invoice_id = parse_public_id(IdKind::Invoice, &id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);
    let idem_scope = format!("invoice.issue.{id}");

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_issue(),
        &request_id,
    )?;

    if body.fx_rate_num <= 0 || body.fx_rate_den <= 0 {
        return Err(validation(&request_id, "fx_rate_num/den must be positive"));
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

    let row = fetch_invoice_row(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;

    enforce_invoice_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::finance_invoice_issue(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if row.status != "draft" {
        return Err(conflict(&request_id, "only draft invoices can be issued"));
    }
    if row.total_minor <= 0 {
        return Err(validation(&request_id, "cannot issue zero-total invoice"));
    }

    let issue_date = parse_date(body.issue_date.as_deref(), "issue_date", &request_id)?
        .unwrap_or_else(|| Utc::now().date_naive());
    let due_date = parse_date(body.due_date.as_deref(), "due_date", &request_id)?.or(row.due_date);
    let fx_date = parse_date(body.fx_rate_date.as_deref(), "fx_rate_date", &request_id)?
        .unwrap_or(issue_date);

    let base_currency = Currency::new(&row.base_currency)
        .map_err(|e| validation(&request_id, format!("invalid base_currency: {e}")))?;

    let invoice_number = next_invoice_number(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    // Resolve tax group/rate refs as of issue date and snapshot into tax_rate_bps.
    let (tax_minor, total_minor) =
        snapshot_taxes_at_issue(&mut tx, org_id, invoice_id, issue_date, &row, &request_id).await?;
    if total_minor <= 0 {
        return Err(validation(&request_id, "cannot issue zero-total invoice"));
    }

    let base_total = convert_to_base(
        total_minor,
        body.fx_rate_num,
        body.fx_rate_den,
        base_currency,
    )
    .map_err(|e| validation(&request_id, format!("fx conversion failed: {e}")))?;

    let currency = Currency::new(&row.currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let net = total_minor - tax_minor;
    let mut journal = invoice_issue_entry(invoice_id, currency, net, tax_minor, total_minor)
        .map_err(|e| validation(&request_id, format!("journal: {e}")))?;
    journal.entity_id = row.entity_id;
    journal.entry_date = Some(issue_date);
    post_journal(&mut tx, org_id, &journal, &request_id).await?;

    sqlx::query(
        r#"
        UPDATE finance_invoice SET
            status = 'issued',
            invoice_number = $3,
            fx_rate_num = $4,
            fx_rate_den = $5,
            fx_rate_date = $6,
            base_total_minor = $7,
            tax_minor = $10,
            total_minor = $11,
            balance_minor = $11,
            issue_date = $8,
            due_date = COALESCE($9, due_date),
            version = version + 1,
            updated_at = now()
        WHERE org_id = $1 AND id = $2 AND status = 'draft'
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .bind(&invoice_number)
    .bind(body.fx_rate_num)
    .bind(body.fx_rate_den)
    .bind(fx_date)
    .bind(base_total)
    .bind(issue_date)
    .bind(due_date)
    .bind(tax_minor)
    .bind(total_minor)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "invoice",
        "issued",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": row.public_id,
            "invoice_number": invoice_number,
            "total_minor": row.total_minor,
            "currency": row.currency,
            "base_total_minor": base_total,
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
        "finance.invoice.issue",
        "invoice",
        &row.public_id,
        serde_json::json!({ "invoice_number": invoice_number }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_invoice_dto(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;

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

/// POST /api/v1/finance/invoices/{id}/send
#[utoipa::path(post, path = "/api/v1/finance/invoices/{id}/send", tag = "finance-invoices",
    responses((status = 200, body = InvoiceDto)))]
pub async fn send_invoice(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<InvoiceDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let invoice_id = parse_public_id(IdKind::Invoice, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_send(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let row = fetch_invoice_row(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;

    enforce_invoice_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::finance_invoice_send(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if !matches!(row.status.as_str(), "issued" | "sent") {
        return Err(conflict(
            &request_id,
            "only issued invoices can be marked sent",
        ));
    }

    let payment_url = format!("/pay/{}", row.public_id);
    sqlx::query(
        r#"
        UPDATE finance_invoice SET
            status = 'sent', sent_at = now(), payment_url = $3,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .bind(&payment_url)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "invoice",
        "sent",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": row.public_id, "payment_url": payment_url }),
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
        "finance.invoice.send",
        "invoice",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_invoice_dto(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/finance/invoices/{id}/void
#[utoipa::path(post, path = "/api/v1/finance/invoices/{id}/void", tag = "finance-invoices",
    responses((status = 200, body = InvoiceDto)))]
pub async fn void_invoice(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<InvoiceDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let invoice_id = parse_public_id(IdKind::Invoice, &id, &request_id)?;

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_void(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let row = fetch_invoice_row(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;

    enforce_invoice_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::finance_invoice_void(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if matches!(row.status.as_str(), "void" | "paid") {
        return Err(conflict(
            &request_id,
            format!("cannot void invoice in status {}", row.status),
        ));
    }
    if row.amount_paid_minor > 0 {
        return Err(conflict(
            &request_id,
            "cannot void invoice with payments applied; issue a credit note",
        ));
    }

    sqlx::query(
        r#"
        UPDATE finance_invoice SET
            status = 'void', voided_at = now(), balance_minor = 0,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(invoice_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "invoice",
        "voided",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": row.public_id }),
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
        "finance.invoice.void",
        "invoice",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_invoice_dto(&mut tx, org_id, invoice_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "invoice"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/finance/invoices/from-quote — snapshot only, no CRM table reads.
#[utoipa::path(post, path = "/api/v1/finance/invoices/from-quote", tag = "finance-invoices",
    request_body = CreateInvoiceFromQuoteRequest,
    responses((status = 201, body = InvoiceDto)))]
pub async fn create_from_quote(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateInvoiceFromQuoteRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope_for(&state.pool, &auth, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_create(),
        &request_id,
    )?;

    if body.lines.is_empty() {
        return Err(validation(&request_id, "quote snapshot requires lines"));
    }
    let _: Uuid = parse_public_id(IdKind::Quote, &body.quote_id, &request_id)?;
    let _: Uuid = parse_public_id(IdKind::Customer, &body.customer_id, &request_id)?;
    let currency = Currency::new(&body.currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;

    let lines: Vec<InvoiceLineInput> = body
        .lines
        .iter()
        .map(|l| InvoiceLineInput {
            description: l.description.clone(),
            quantity: l.quantity,
            unit_price_minor: l.unit_price_minor,
            discount_minor: l.discount_minor,
            tax_rate_bps: l.tax_rate_bps,
            tax_rate_id: None,
            tax_group_id: None,
        })
        .collect();
    let inputs = line_inputs(&lines);
    let (computed, doc) = compute_document_totals(&inputs, currency)
        .map_err(|e| validation(&request_id, format!("invalid line totals: {e}")))?;

    let public_id = PublicId::generate(IdKind::Invoice);
    let id = public_id.uuid();
    let snapshot = serde_json::to_value(&body).unwrap_or_default();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "invoice.from_quote", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let customer_id = ensure_customer_from_snapshot(
        &mut tx,
        auth.ctx.org_id,
        &body.customer_id,
        &body.customer_name,
        &body.currency,
    )
    .await
    .map_err(internal(&request_id))?;
    let (entity_uuid, _) = resolve_entity_id(&mut tx, org_id, None, &request_id).await?;

    sqlx::query(
        r#"
        INSERT INTO finance_invoice (
            id, org_id, public_id, customer_id, owner_user_id, status,
            currency, base_currency, subtotal_minor, discount_minor, tax_minor,
            total_minor, base_total_minor, balance_minor, notes, terms,
            source_quote_public_id, source_quote_snapshot, entity_id
        ) VALUES ($1,$2,$3,$4,$5,'draft',$6,'USD',$7,$8,$9,$10,0,0,$11,$12,$13,$14,$15)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(customer_id)
    .bind(auth.ctx.actor.user_id)
    .bind(&body.currency)
    .bind(doc.subtotal_minor)
    .bind(doc.discount_minor)
    .bind(doc.tax_minor)
    .bind(doc.total_minor)
    .bind(&body.notes)
    .bind(&body.terms)
    .bind(&body.quote_id)
    .bind(&snapshot)
    .bind(entity_uuid)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_lines(&mut tx, org_id, id, &lines, &computed, &request_id).await?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "invoice",
        "created_from_quote",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "source_quote_id": body.quote_id,
            "total_minor": doc.total_minor,
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
        "finance.invoice.from_quote",
        "invoice",
        &public_id.as_str(),
        serde_json::json!({ "quote_id": body.quote_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_invoice_dto(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "invoice missing after insert",
            )
        })?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "invoice.from_quote",
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
