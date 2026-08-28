//! Journal posting + list/get — payroll, manual, reversals.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::new_uuid_v7;
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use serde_json::json;
use uuid::Uuid;

use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::handlers::{internal, normalize_paging, not_found, parse_public_id, validation};
use crate::idempotency;
use crate::journal::{self, JournalDraft, LedgerLine};
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    JournalEntryDto, JournalLineInput, JournalListQuery, JournalListResponse, PostJournalRequest,
};
use companyos_ids::IdKind;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/journals",
            get(list_journals).post(post_journal_handler),
        )
        .route("/api/v1/finance/journals/{id}", get(get_journal))
}

fn parse_journal_public_id(raw: &str, request_id: &str) -> Result<Uuid, AppError> {
    let s = raw.trim();
    let uuid_str = s.strip_prefix("jrn_").unwrap_or(s);
    Uuid::parse_str(uuid_str)
        .map_err(|_| validation(request_id, format!("invalid journal id: {raw}")))
}

async fn resolve_active_account_code(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    code: &str,
    request_id: &str,
) -> Result<String, AppError> {
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT code, is_active FROM finance_ledger_account WHERE org_id = $1 AND code = $2",
    )
    .bind(org_id)
    .bind(code)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    match row {
        Some((c, true)) => Ok(c),
        Some((_, false)) => Err(validation(
            request_id,
            format!("account {code} is inactive"),
        )),
        None => Err(validation(
            request_id,
            format!("unknown account_code: {code}"),
        )),
    }
}

async fn load_entry_dto(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    entry_id: Uuid,
) -> Result<Option<JournalEntryDto>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let row: Option<(
        String,
        String,
        String,
        Uuid,
        String,
        NaiveDate,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT je.public_id, je.memo, je.source_type, je.source_id, je.currency,
               je.entry_date, fp.public_id
        FROM finance_journal_entry je
        LEFT JOIN finance_fiscal_period fp ON fp.id = je.period_id
        WHERE je.org_id = $1 AND je.id = $2
        "#,
    )
    .bind(org_id)
    .bind(entry_id)
    .fetch_optional(&mut **tx)
    .await?;

    let Some((public_id, memo, st, source_id, currency, entry_date, period_id)) = row else {
        return Ok(None);
    };

    let lines: Vec<(String, i64, i64, Option<String>)> = sqlx::query_as(
        r#"
        SELECT a.code, jl.debit_minor, jl.credit_minor, jl.memo
        FROM finance_journal_line jl
        JOIN finance_ledger_account a ON a.id = jl.account_id
        WHERE jl.org_id = $1 AND jl.entry_id = $2
        ORDER BY jl.created_at
        "#,
    )
    .bind(org_id)
    .bind(entry_id)
    .fetch_all(&mut **tx)
    .await?;

    Ok(Some(JournalEntryDto {
        id: public_id,
        memo,
        source_type: st,
        source_id: source_id.to_string(),
        currency,
        entry_date: entry_date.to_string(),
        period_id,
        lines: lines
            .into_iter()
            .map(
                |(account_code, debit_minor, credit_minor, memo)| JournalLineInput {
                    account_code,
                    debit_minor,
                    credit_minor,
                    memo,
                },
            )
            .collect(),
    }))
}

async fn fetch_by_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    source_type: &str,
    source_id: Uuid,
) -> Result<Option<JournalEntryDto>, sqlx::Error> {
    let entry_id: Option<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id FROM finance_journal_entry
        WHERE org_id = $1 AND source_type = $2 AND source_id = $3
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(source_type)
    .bind(source_id)
    .fetch_optional(&mut **tx)
    .await?;
    match entry_id {
        Some(id) => load_entry_dto(tx, org_id, id).await,
        None => Ok(None),
    }
}

/// GET /api/v1/finance/journals
#[utoipa::path(
    get,
    path = "/api/v1/finance/journals",
    tag = "finance-journals",
    params(
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
        ("source_type" = Option<String>, Query),
        ("period_id" = Option<String>, Query),
    ),
    responses((status = 200, body = JournalListResponse))
)]
pub async fn list_journals(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<JournalListQuery>,
) -> Result<Json<JournalListResponse>, AppError> {
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
        perms::finance_ledger_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let period_uuid = if let Some(ref pid) = q.period_id {
        Some(parse_public_id(IdKind::FiscalPeriod, pid, &request_id)?)
    } else {
        None
    };

    let total: i64 = match (q.source_type.as_deref(), period_uuid) {
        (Some(st), Some(pid)) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM finance_journal_entry WHERE org_id = $1 AND source_type = $2 AND period_id = $3",
        )
        .bind(org_id)
        .bind(st)
        .bind(pid)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?,
        (Some(st), None) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM finance_journal_entry WHERE org_id = $1 AND source_type = $2",
        )
        .bind(org_id)
        .bind(st)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?,
        (None, Some(pid)) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM finance_journal_entry WHERE org_id = $1 AND period_id = $2",
        )
        .bind(org_id)
        .bind(pid)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?,
        (None, None) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM finance_journal_entry WHERE org_id = $1",
        )
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?,
    };

    let entry_ids: Vec<(Uuid,)> = match (q.source_type.as_deref(), period_uuid) {
        (Some(st), Some(pid)) => sqlx::query_as(
            "SELECT id FROM finance_journal_entry WHERE org_id = $1 AND source_type = $2 AND period_id = $3
             ORDER BY entry_date DESC, created_at DESC LIMIT $4 OFFSET $5",
        )
        .bind(org_id)
        .bind(st)
        .bind(pid)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?,
        (Some(st), None) => sqlx::query_as(
            "SELECT id FROM finance_journal_entry WHERE org_id = $1 AND source_type = $2
             ORDER BY entry_date DESC, created_at DESC LIMIT $3 OFFSET $4",
        )
        .bind(org_id)
        .bind(st)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?,
        (None, Some(pid)) => sqlx::query_as(
            "SELECT id FROM finance_journal_entry WHERE org_id = $1 AND period_id = $2
             ORDER BY entry_date DESC, created_at DESC LIMIT $3 OFFSET $4",
        )
        .bind(org_id)
        .bind(pid)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?,
        (None, None) => sqlx::query_as(
            "SELECT id FROM finance_journal_entry WHERE org_id = $1
             ORDER BY entry_date DESC, created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(org_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?,
    };

    let mut items = Vec::with_capacity(entry_ids.len());
    for (eid,) in entry_ids {
        if let Some(dto) = load_entry_dto(&mut tx, org_id, eid)
            .await
            .map_err(internal(&request_id))?
        {
            items.push(dto);
        }
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(JournalListResponse { items, total }))
}

/// GET /api/v1/finance/journals/{id}
#[utoipa::path(
    get,
    path = "/api/v1/finance/journals/{id}",
    tag = "finance-journals",
    responses((status = 200, body = JournalEntryDto))
)]
pub async fn get_journal(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<JournalEntryDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let entry_id = parse_journal_public_id(&id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_ledger_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let dto = load_entry_dto(&mut tx, org_id, entry_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "journal"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST /api/v1/finance/journals
#[utoipa::path(
    post,
    path = "/api/v1/finance/journals",
    tag = "finance-journals",
    request_body = PostJournalRequest,
    responses((status = 201, body = JournalEntryDto), (status = 200, body = JournalEntryDto), (status = 403), (status = 422))
)]
pub async fn post_journal_handler(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<PostJournalRequest>,
) -> Result<impl IntoResponse, AppError> {
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
        perms::finance_journal_post(),
        &request_id,
    )?;

    const ALLOWED_SOURCE_TYPES: &[&str] = &[
        "payroll",
        "manual",
        "inventory_receipt",
        "inventory_cogs",
        "inventory_depreciation",
        "vendor_bill",
        "vendor_payment",
    ];
    let source_type = body.source_type.trim();
    if !ALLOWED_SOURCE_TYPES.contains(&source_type) {
        return Err(validation(
            &request_id,
            format!(
                "source_type must be one of: {}",
                ALLOWED_SOURCE_TYPES.join(", ")
            ),
        ));
    }

    let currency: Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;

    let source_id = if body.source_id.trim().is_empty() {
        if source_type == "manual" {
            new_uuid_v7()
        } else {
            return Err(validation(
                &request_id,
                "source_id must be a UUID for payroll",
            ));
        }
    } else {
        Uuid::parse_str(body.source_id.trim())
            .map_err(|_| validation(&request_id, "source_id must be a UUID"))?
    };

    let entry_date = match body.entry_date.as_deref() {
        Some(s) => Some(
            NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
                .map_err(|_| validation(&request_id, "entry_date must be YYYY-MM-DD"))?,
        ),
        None => None,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    journal::ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let reverses_entry_id = if let Some(ref raw) = body.reverses_of {
        Some(parse_journal_public_id(raw, &request_id)?)
    } else {
        None
    };

    let mut lines = Vec::new();
    let mut response_lines = Vec::new();

    if let Some(orig_id) = reverses_entry_id {
        let orig = load_entry_dto(&mut tx, org_id, orig_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "journal to reverse"))?;
        for line in &orig.lines {
            let account_code =
                resolve_active_account_code(&mut tx, org_id, &line.account_code, &request_id)
                    .await?;
            // Invert debit/credit.
            let debit = line.credit_minor;
            let credit = line.debit_minor;
            lines.push(LedgerLine {
                account_code: account_code.clone(),
                debit_minor: debit,
                credit_minor: credit,
                memo: line.memo.clone(),
            });
            response_lines.push(JournalLineInput {
                account_code,
                debit_minor: debit,
                credit_minor: credit,
                memo: line.memo.clone(),
            });
        }
        // Allow additional explicit lines on top of the reversal.
        for line in &body.lines {
            if (line.debit_minor > 0 && line.credit_minor > 0)
                || (line.debit_minor == 0 && line.credit_minor == 0)
                || line.debit_minor < 0
                || line.credit_minor < 0
            {
                return Err(validation(
                    &request_id,
                    "each line must have exactly one of debit_minor or credit_minor > 0",
                ));
            }
            let account_code =
                resolve_active_account_code(&mut tx, org_id, &line.account_code, &request_id)
                    .await?;
            lines.push(LedgerLine {
                account_code: account_code.clone(),
                debit_minor: line.debit_minor,
                credit_minor: line.credit_minor,
                memo: line.memo.clone(),
            });
            response_lines.push(JournalLineInput {
                account_code,
                debit_minor: line.debit_minor,
                credit_minor: line.credit_minor,
                memo: line.memo.clone(),
            });
        }
    } else {
        if body.lines.is_empty() {
            return Err(validation(&request_id, "lines required"));
        }
        for line in &body.lines {
            if (line.debit_minor > 0 && line.credit_minor > 0)
                || (line.debit_minor == 0 && line.credit_minor == 0)
                || line.debit_minor < 0
                || line.credit_minor < 0
            {
                return Err(validation(
                    &request_id,
                    "each line must have exactly one of debit_minor or credit_minor > 0",
                ));
            }
            let account_code =
                resolve_active_account_code(&mut tx, org_id, &line.account_code, &request_id)
                    .await?;
            lines.push(LedgerLine {
                account_code: account_code.clone(),
                debit_minor: line.debit_minor,
                credit_minor: line.credit_minor,
                memo: line.memo.clone(),
            });
            response_lines.push(JournalLineInput {
                account_code,
                debit_minor: line.debit_minor,
                credit_minor: line.credit_minor,
                memo: line.memo.clone(),
            });
        }
    }

    if lines.is_empty() {
        return Err(validation(&request_id, "lines required"));
    }

    let default_memo = match reverses_entry_id {
        Some(rid) => format!("Reversal of jrn_{rid}"),
        None => format!("{source_type} journal for {source_id}"),
    };
    let source_type_static = ALLOWED_SOURCE_TYPES
        .iter()
        .find(|s| **s == source_type)
        .copied()
        .unwrap_or("manual");
    let draft = JournalDraft {
        memo: body.memo.clone().unwrap_or(default_memo),
        source_type: source_type_static,
        source_id,
        currency,
        lines,
        entry_date,
        reverses_entry_id,
        posted_by: Some(auth.ctx.actor.user_id),
    };
    draft
        .assert_balanced()
        .map_err(|_| validation(&request_id, "journal lines must balance"))?;

    let idem_key = idempotency::header_key(&headers);

    if let Some(ref key) = idem_key {
        if let Some((_status, prev)) = idempotency::get(&mut *tx, org_id, "journal.post", key)
            .await
            .map_err(internal(&request_id))?
        {
            let dto: JournalEntryDto = serde_json::from_value(prev).map_err(|e| {
                AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
            })?;
            return Ok((StatusCode::OK, Json(dto)));
        }
    }

    // Idempotent on (org, source_type, source_id) — return existing entry.
    if let Some(existing) = fetch_by_source(&mut tx, org_id, draft.source_type, source_id)
        .await
        .map_err(internal(&request_id))?
    {
        if let Some(ref key) = idem_key {
            let body_json = serde_json::to_value(&existing).unwrap_or(json!({}));
            idempotency::put(&mut *tx, org_id, "journal.post", key, 200, body_json)
                .await
                .map_err(internal(&request_id))?;
        }
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((StatusCode::OK, Json(existing)));
    }

    let entry_id = journal::post_journal(&mut tx, org_id, &draft, &request_id).await?;
    let dto = load_entry_dto(&mut tx, org_id, entry_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "journal missing after post",
            )
        })?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.journal.post",
        "journal_entry",
        &dto.id,
        json!({
            "source_type": draft.source_type,
            "source_id": source_id.to_string(),
            "line_count": draft.lines.len(),
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "journal",
        "posted",
        1,
        auth.ctx.actor.clone(),
        json!({
            "id": dto.id,
            "source_type": draft.source_type,
            "source_id": source_id.to_string(),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(ref key) = idem_key {
        let body_json = serde_json::to_value(&dto).unwrap_or(json!({}));
        idempotency::put(&mut *tx, org_id, "journal.post", key, 201, body_json)
            .await
            .map_err(internal(&request_id))?;
    }

    // Suppress unused warning if response_lines built but dto reloaded from DB.
    let _ = response_lines;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}
