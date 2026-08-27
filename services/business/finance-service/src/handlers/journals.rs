//! `POST /api/v1/finance/journals` — post balanced journals (payroll + future callers).
//!
//! Cross-context: People/HR calls this with `on_behalf_of` and never inserts
//! into `finance_*` tables directly.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use serde_json::json;
use uuid::Uuid;

use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::handlers::{internal, validation};
use crate::idempotency;
use crate::journal::{self, JournalDraft, LedgerLine};
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{JournalEntryDto, JournalLineInput, PostJournalRequest};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/finance/journals", post(post_journal_handler))
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

    let source_type = body.source_type.trim();
    if source_type != "payroll" && source_type != "manual" {
        return Err(validation(
            &request_id,
            "source_type must be payroll or manual",
        ));
    }

    let currency: Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;

    let source_id = Uuid::parse_str(body.source_id.trim())
        .map_err(|_| validation(&request_id, "source_id must be a UUID"))?;

    if body.lines.is_empty() {
        return Err(validation(&request_id, "lines required"));
    }

    let mut lines = Vec::with_capacity(body.lines.len());
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
        let account_code = match line.account_code.as_str() {
            "1000" => journal::codes::CASH,
            "1100" => journal::codes::AR,
            "2100" => journal::codes::TAX_PAYABLE,
            "2200" => journal::codes::CUSTOMER_CREDITS,
            "2300" => journal::codes::PAYROLL_DEDUCTIONS,
            "2400" => journal::codes::NET_PAY_CLEARING,
            "4000" => journal::codes::REVENUE,
            "5000" => journal::codes::EXPENSE,
            "5100" => journal::codes::WAGES_EXPENSE,
            other => {
                return Err(validation(
                    &request_id,
                    format!("unknown account_code: {other}"),
                ));
            }
        };
        lines.push(LedgerLine {
            account_code,
            debit_minor: line.debit_minor,
            credit_minor: line.credit_minor,
            memo: line.memo.clone(),
        });
    }

    let draft = JournalDraft {
        memo: body.memo.clone().unwrap_or_else(|| {
            format!("{source_type} journal for {source_id}")
        }),
        source_type: if source_type == "payroll" {
            "payroll"
        } else {
            "manual"
        },
        source_id,
        currency,
        lines,
    };
    draft
        .assert_balanced()
        .map_err(|_| validation(&request_id, "journal lines must balance"))?;

    let idem_key = idempotency::header_key(&headers);
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(ref key) = idem_key {
        if let Some((status, prev)) =
            idempotency::get(&mut *tx, org_id, "journal.post", key)
                .await
                .map_err(internal(&request_id))?
        {
            let dto: JournalEntryDto = serde_json::from_value(prev).map_err(|e| {
                AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
            })?;
            return Ok((StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK), Json(dto)));
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

    journal::ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;
    let entry_id = journal::post_journal(&mut tx, org_id, &draft)
        .await
        .map_err(internal(&request_id))?;

    let public_id = format!("jrn_{entry_id}");
    let dto = JournalEntryDto {
        id: public_id.clone(),
        memo: draft.memo.clone(),
        source_type: draft.source_type.to_string(),
        source_id: source_id.to_string(),
        currency: currency.as_str().to_string(),
        lines: body.lines.clone(),
    };

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.journal.post",
        "journal_entry",
        &public_id,
        json!({
            "source_type": draft.source_type,
            "source_id": source_id.to_string(),
            // Do not include amount figures in audit metadata logs path —
            // store only line count.
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
            "id": public_id,
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

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn fetch_by_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    source_type: &str,
    source_id: Uuid,
) -> Result<Option<JournalEntryDto>, sqlx::Error> {
    let row: Option<(Uuid, String, String, String, String)> = sqlx::query_as(
        r#"
        SELECT id, public_id, memo, source_type, currency
        FROM finance_journal_entry
        WHERE org_id = $1 AND source_type = $2 AND source_id = $3
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(source_type)
    .bind(source_id)
    .fetch_optional(&mut **tx)
    .await?;

    let Some((entry_id, public_id, memo, st, currency)) = row else {
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
        lines: lines
            .into_iter()
            .map(|(account_code, debit_minor, credit_minor, memo)| JournalLineInput {
                account_code,
                debit_minor,
                credit_minor,
                memo,
            })
            .collect(),
    }))
}
