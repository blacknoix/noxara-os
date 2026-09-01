//! Intercompany transactions + consolidation runs (Phase 4.2).
//!
//! Same-currency first. An intercompany pair posts balanced journals on both
//! entities (IC receivable / payable + IC revenue / expense). Consolidation
//! sums entity TBs and eliminates the IC pair to net zero.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{NaiveDate, Utc};
use companyos_authz::{is_allowed, perms};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

use super::{internal, parse_public_id, validation};
use crate::auth::AuthCtx;
use crate::journal::{self, codes, JournalDraft, LedgerLine};
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{TrialBalanceResponse, TrialBalanceRow};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/intercompany",
            get(list_intercompany).post(create_intercompany),
        )
        .route(
            "/api/v1/finance/entities/{id}/access",
            post(grant_entity_access),
        )
        .route(
            "/api/v1/finance/consolidation/runs",
            get(list_runs).post(run_consolidation),
        )
        .route("/api/v1/finance/consolidation/runs/{id}", get(get_run))
}

#[derive(Debug, Deserialize)]
pub struct CreateIntercompanyRequest {
    pub from_entity_id: String,
    pub to_entity_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub memo: String,
}

#[derive(Debug, Serialize)]
pub struct IntercompanyDto {
    pub id: String,
    pub from_entity_id: String,
    pub to_entity_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub memo: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct GrantAccessRequest {
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ConsolidationRequest {
    pub entity_ids: Option<Vec<String>>,
    pub currency: Option<String>,
    pub as_of: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PnLSummary {
    pub revenue_minor: i64,
    pub expense_minor: i64,
    pub net_income_minor: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BsSummary {
    pub assets_minor: i64,
    pub liabilities_minor: i64,
    pub equity_minor: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EntityStatementPack {
    pub entity_id: String,
    pub entity_code: String,
    pub trial_balance: TrialBalanceResponse,
    pub profit_and_loss: PnLSummary,
    pub balance_sheet: BsSummary,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EliminationLine {
    pub account_code: String,
    pub debit_minor: i64,
    pub credit_minor: i64,
    pub memo: String,
}

#[derive(Debug, Serialize)]
pub struct ConsolidationRunDto {
    pub id: String,
    pub currency: String,
    pub as_of: String,
    pub status: String,
    pub eliminated_minor: i64,
    pub entity_statements: Vec<EntityStatementPack>,
    pub consolidated_trial_balance: TrialBalanceResponse,
    pub eliminations: Vec<EliminationLine>,
}

async fn create_intercompany(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateIntercompanyRequest>,
) -> Result<Json<IntercompanyDto>, AppError> {
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
        perms::finance_intercompany_manage(),
        &request_id,
    )?;

    if body.amount_minor <= 0 {
        return Err(validation(&request_id, "amount_minor must be positive"));
    }
    let currency: Currency = body
        .currency
        .parse()
        .map_err(|_| validation(&request_id, "invalid currency"))?;

    let from_id = parse_public_id(IdKind::FinanceEntity, &body.from_entity_id, &request_id)?;
    let to_id = parse_public_id(IdKind::FinanceEntity, &body.to_entity_id, &request_id)?;
    if from_id == to_id {
        return Err(validation(&request_id, "from and to entities must differ"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    journal::ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let entities: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, public_id, currency FROM finance_entity WHERE org_id = $1 AND id = ANY($2)",
    )
    .bind(org_id)
    .bind([from_id, to_id])
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    if entities.len() != 2 {
        return Err(validation(&request_id, "entity not found"));
    }
    for (_, _, ent_cur) in &entities {
        if ent_cur != body.currency.as_str() {
            return Err(validation(
                &request_id,
                "same-currency only: entity currency must match transaction",
            ));
        }
    }
    let from_public = entities
        .iter()
        .find(|(id, _, _)| *id == from_id)
        .map(|(_, p, _)| p.clone())
        .unwrap();
    let to_public = entities
        .iter()
        .find(|(id, _, _)| *id == to_id)
        .map(|(_, p, _)| p.clone())
        .unwrap();

    let txn_id = new_uuid_v7();
    let txn_public = PublicId::new(IdKind::IntercompanyTxn, txn_id);

    let from_draft = JournalDraft {
        memo: format!("IC {}", body.memo),
        source_type: "intercompany",
        source_id: txn_id,
        currency,
        lines: vec![
            LedgerLine::debit(
                codes::IC_EXPENSE,
                body.amount_minor,
                Some(body.memo.clone()),
            ),
            LedgerLine::credit(
                codes::IC_PAYABLE,
                body.amount_minor,
                Some(to_public.clone()),
            ),
        ],
        entry_date: Some(Utc::now().date_naive()),
        reverses_entry_id: None,
        posted_by: Some(auth.ctx.actor.user_id),
        entity_id: Some(from_id),
    };
    let from_journal = journal::post_journal(&mut tx, org_id, &from_draft, &request_id).await?;

    let to_draft = JournalDraft {
        memo: format!("IC {}", body.memo),
        source_type: "intercompany",
        source_id: txn_id,
        currency,
        lines: vec![
            LedgerLine::debit(
                codes::IC_RECEIVABLE,
                body.amount_minor,
                Some(from_public.clone()),
            ),
            LedgerLine::credit(
                codes::IC_REVENUE,
                body.amount_minor,
                Some(body.memo.clone()),
            ),
        ],
        entry_date: Some(Utc::now().date_naive()),
        reverses_entry_id: None,
        posted_by: Some(auth.ctx.actor.user_id),
        entity_id: Some(to_id),
    };
    let to_journal = journal::post_journal(&mut tx, org_id, &to_draft, &request_id).await?;

    sqlx::query(
        r#"
        INSERT INTO finance_intercompany_txn (
            id, org_id, public_id, from_entity_id, to_entity_id, currency, amount_minor,
            memo, status, from_journal_id, to_journal_id, posted_by, posted_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'posted',$9,$10,$11,now())
        "#,
    )
    .bind(txn_id)
    .bind(org_id)
    .bind(txn_public.as_str())
    .bind(from_id)
    .bind(to_id)
    .bind(body.currency.as_str())
    .bind(body.amount_minor)
    .bind(&body.memo)
    .bind(from_journal)
    .bind(to_journal)
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(IntercompanyDto {
        id: txn_public.to_string(),
        from_entity_id: from_public,
        to_entity_id: to_public,
        amount_minor: body.amount_minor,
        currency: body.currency,
        memo: body.memo,
        status: "posted".into(),
    }))
}

async fn list_intercompany(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<Vec<IntercompanyDto>>, AppError> {
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
        perms::finance_consolidation_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<(String, String, String, i64, String, String, String)> = sqlx::query_as(
        r#"
        SELECT t.public_id, f.public_id, e.public_id, t.amount_minor, t.currency, t.memo, t.status
        FROM finance_intercompany_txn t
        JOIN finance_entity f ON f.id = t.from_entity_id
        JOIN finance_entity e ON e.id = t.to_entity_id
        WHERE t.org_id = $1
        ORDER BY t.created_at DESC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(id, from_entity_id, to_entity_id, amount_minor, currency, memo, status)| {
                    IntercompanyDto {
                        id,
                        from_entity_id,
                        to_entity_id,
                        amount_minor,
                        currency,
                        memo,
                        status,
                    }
                },
            )
            .collect(),
    ))
}

async fn grant_entity_access(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<GrantAccessRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
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
        perms::finance_entity_manage(),
        &request_id,
    )?;

    let entity_id = parse_public_id(IdKind::FinanceEntity, &id, &request_id)?;
    let user_pid: PublicId = body
        .user_id
        .parse()
        .map_err(|_| validation(&request_id, "invalid user_id"))?;
    if user_pid.kind() != IdKind::User {
        return Err(validation(&request_id, "user_id must be usr_…"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO finance_entity_access (id, org_id, entity_id, user_id)
        VALUES ($1,$2,$3,$4)
        ON CONFLICT (org_id, entity_id, user_id) DO NOTHING
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id)
    .bind(entity_id)
    .bind(user_pid.uuid())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(
        json!({ "granted": true, "entity_id": id, "user_id": body.user_id }),
    ))
}

async fn entity_tb(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    entity_id: Uuid,
    currency: &str,
) -> Result<TrialBalanceResponse, AppError> {
    #[derive(sqlx::FromRow)]
    struct TbRow {
        account_code: String,
        account_name: String,
        account_type: String,
        debit_minor: i64,
        credit_minor: i64,
    }
    let rows: Vec<TbRow> = sqlx::query_as(
        r#"
        SELECT a.code AS account_code, a.name AS account_name, a.account_type,
               COALESCE(SUM(jl.debit_minor), 0)::BIGINT AS debit_minor,
               COALESCE(SUM(jl.credit_minor), 0)::BIGINT AS credit_minor
        FROM finance_journal_line jl
        JOIN finance_journal_entry je ON je.id = jl.entry_id
        JOIN finance_ledger_account a ON a.id = jl.account_id
        WHERE jl.org_id = $1 AND je.currency = $2 AND je.entity_id = $3
        GROUP BY a.code, a.name, a.account_type
        HAVING COALESCE(SUM(jl.debit_minor), 0) <> 0
            OR COALESCE(SUM(jl.credit_minor), 0) <> 0
        ORDER BY a.code
        "#,
    )
    .bind(org_id)
    .bind(currency)
    .bind(entity_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, "tb", e.to_string()))?;

    let total_debit_minor: i64 = rows.iter().map(|r| r.debit_minor).sum();
    let total_credit_minor: i64 = rows.iter().map(|r| r.credit_minor).sum();
    Ok(TrialBalanceResponse {
        currency: currency.to_string(),
        period_id: None,
        rows: rows
            .into_iter()
            .map(|r| TrialBalanceRow {
                account_code: r.account_code,
                account_name: r.account_name,
                account_type: r.account_type,
                debit_minor: r.debit_minor,
                credit_minor: r.credit_minor,
            })
            .collect(),
        total_debit_minor,
        total_credit_minor,
        balanced: total_debit_minor == total_credit_minor,
    })
}

fn pnl_from_tb(tb: &TrialBalanceResponse) -> PnLSummary {
    let mut revenue_minor = 0i64;
    let mut expense_minor = 0i64;
    for r in &tb.rows {
        match r.account_type.as_str() {
            "revenue" => revenue_minor += r.credit_minor - r.debit_minor,
            "expense" => expense_minor += r.debit_minor - r.credit_minor,
            _ => {}
        }
    }
    PnLSummary {
        revenue_minor,
        expense_minor,
        net_income_minor: revenue_minor - expense_minor,
    }
}

fn bs_from_tb(tb: &TrialBalanceResponse) -> BsSummary {
    let mut assets_minor = 0i64;
    let mut liabilities_minor = 0i64;
    let mut equity_minor = 0i64;
    for r in &tb.rows {
        let net = r.debit_minor - r.credit_minor;
        match r.account_type.as_str() {
            "asset" => assets_minor += net,
            "liability" => liabilities_minor += -net,
            "equity" => equity_minor += -net,
            _ => {}
        }
    }
    // Roll P&L into equity for BS presentation.
    let pnl = pnl_from_tb(tb);
    equity_minor += pnl.net_income_minor;
    BsSummary {
        assets_minor,
        liabilities_minor,
        equity_minor,
    }
}

async fn run_consolidation(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ConsolidationRequest>,
) -> Result<Json<ConsolidationRunDto>, AppError> {
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
        perms::finance_consolidation_run(),
        &request_id,
    )?;

    let currency = body.currency.unwrap_or_else(|| "USD".into());
    let as_of: NaiveDate = body
        .as_of
        .as_deref()
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| validation(&request_id, "as_of must be YYYY-MM-DD"))
        })
        .transpose()?
        .unwrap_or_else(|| Utc::now().date_naive());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let entities: Vec<(Uuid, String, String)> = if let Some(ref ids) = body.entity_ids {
        let mut out = Vec::new();
        for id in ids {
            let uuid = parse_public_id(IdKind::FinanceEntity, id, &request_id)?;
            let row: Option<(Uuid, String, String)> = sqlx::query_as(
                "SELECT id, public_id, code FROM finance_entity WHERE org_id = $1 AND id = $2",
            )
            .bind(org_id)
            .bind(uuid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            let Some(r) = row else {
                return Err(validation(&request_id, format!("entity not found: {id}")));
            };
            out.push(r);
        }
        out
    } else {
        sqlx::query_as(
            "SELECT id, public_id, code FROM finance_entity WHERE org_id = $1 ORDER BY code",
        )
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?
    };

    let mut entity_statements = Vec::new();
    let mut combined: HashMap<String, (String, String, i64, i64)> = HashMap::new();

    for (eid, public_id, code) in &entities {
        let tb = entity_tb(&mut tx, org_id, *eid, &currency).await?;
        if !tb.balanced {
            return Err(AppError::new(
                ErrorCode::Conflict,
                request_id.clone(),
                format!("entity {code} trial balance does not balance"),
            ));
        }
        for r in &tb.rows {
            let entry = combined
                .entry(r.account_code.clone())
                .or_insert_with(|| (r.account_name.clone(), r.account_type.clone(), 0, 0));
            entry.2 += r.debit_minor;
            entry.3 += r.credit_minor;
        }
        let profit_and_loss = pnl_from_tb(&tb);
        let balance_sheet = bs_from_tb(&tb);
        entity_statements.push(EntityStatementPack {
            entity_id: public_id.clone(),
            entity_code: code.clone(),
            trial_balance: tb,
            profit_and_loss,
            balance_sheet,
        });
    }

    // Eliminations: net IC receivable vs payable, IC revenue vs expense.
    let ic_recv = combined
        .get(codes::IC_RECEIVABLE)
        .map(|(_, _, d, c)| d - c)
        .unwrap_or(0);
    let ic_pay = combined
        .get(codes::IC_PAYABLE)
        .map(|(_, _, d, c)| c - d)
        .unwrap_or(0);
    let ic_rev = combined
        .get(codes::IC_REVENUE)
        .map(|(_, _, d, c)| c - d)
        .unwrap_or(0);
    let ic_exp = combined
        .get(codes::IC_EXPENSE)
        .map(|(_, _, d, c)| d - c)
        .unwrap_or(0);

    let elim_bs = ic_recv.min(ic_pay).max(0);
    let elim_pnl = ic_rev.min(ic_exp).max(0);
    let eliminated_minor = elim_bs + elim_pnl;

    let mut eliminations = Vec::new();
    if elim_bs > 0 {
        eliminations.push(EliminationLine {
            account_code: codes::IC_RECEIVABLE.into(),
            debit_minor: 0,
            credit_minor: elim_bs,
            memo: "Eliminate IC receivable".into(),
        });
        eliminations.push(EliminationLine {
            account_code: codes::IC_PAYABLE.into(),
            debit_minor: elim_bs,
            credit_minor: 0,
            memo: "Eliminate IC payable".into(),
        });
        if let Some(e) = combined.get_mut(codes::IC_RECEIVABLE) {
            e.3 += elim_bs;
        }
        if let Some(e) = combined.get_mut(codes::IC_PAYABLE) {
            e.2 += elim_bs;
        }
    }
    if elim_pnl > 0 {
        eliminations.push(EliminationLine {
            account_code: codes::IC_REVENUE.into(),
            debit_minor: elim_pnl,
            credit_minor: 0,
            memo: "Eliminate IC revenue".into(),
        });
        eliminations.push(EliminationLine {
            account_code: codes::IC_EXPENSE.into(),
            debit_minor: 0,
            credit_minor: elim_pnl,
            memo: "Eliminate IC expense".into(),
        });
        if let Some(e) = combined.get_mut(codes::IC_REVENUE) {
            e.2 += elim_pnl;
        }
        if let Some(e) = combined.get_mut(codes::IC_EXPENSE) {
            e.3 += elim_pnl;
        }
    }

    let mut cons_rows: Vec<TrialBalanceRow> = combined
        .into_iter()
        .filter(|(_, (_, _, d, c))| *d != 0 || *c != 0)
        .map(|(code, (name, ty, d, c))| TrialBalanceRow {
            account_code: code,
            account_name: name,
            account_type: ty,
            debit_minor: d,
            credit_minor: c,
        })
        .collect();
    cons_rows.sort_by(|a, b| a.account_code.cmp(&b.account_code));

    // After elimination, IC nets should be zero.
    let ic_net_ok = cons_rows
        .iter()
        .filter(|r| {
            matches!(
                r.account_code.as_str(),
                codes::IC_RECEIVABLE | codes::IC_PAYABLE | codes::IC_REVENUE | codes::IC_EXPENSE
            )
        })
        .all(|r| r.debit_minor == r.credit_minor);

    if !ic_net_ok {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            "intercompany eliminations did not net to zero",
        ));
    }

    // Drop fully-eliminated zero rows for cleaner statement.
    cons_rows.retain(|r| {
        r.debit_minor != r.credit_minor
            || !matches!(
                r.account_code.as_str(),
                codes::IC_RECEIVABLE | codes::IC_PAYABLE | codes::IC_REVENUE | codes::IC_EXPENSE
            )
    });
    // Actually after elim debit==credit for IC; remove those:
    cons_rows.retain(|r| r.debit_minor != r.credit_minor);

    let total_debit_minor: i64 = cons_rows.iter().map(|r| r.debit_minor).sum();
    let total_credit_minor: i64 = cons_rows.iter().map(|r| r.credit_minor).sum();
    let consolidated_trial_balance = TrialBalanceResponse {
        currency: currency.clone(),
        period_id: None,
        rows: cons_rows,
        total_debit_minor,
        total_credit_minor,
        balanced: total_debit_minor == total_credit_minor,
    };

    let run_id = new_uuid_v7();
    let run_public = PublicId::new(IdKind::ConsolidationRun, run_id);
    let entity_uuids: Vec<Uuid> = entities.iter().map(|(id, _, _)| *id).collect();
    let statements = json!({
        "entity_statements": entity_statements,
        "consolidated_trial_balance": consolidated_trial_balance,
        "eliminations": eliminations,
    });

    sqlx::query(
        r#"
        INSERT INTO finance_consolidation_run (
            id, org_id, public_id, currency, as_of, status, entity_ids,
            eliminated_minor, statements, created_by
        ) VALUES ($1,$2,$3,$4,$5,'completed',$6,$7,$8,$9)
        "#,
    )
    .bind(run_id)
    .bind(org_id)
    .bind(run_public.as_str())
    .bind(&currency)
    .bind(as_of)
    .bind(&entity_uuids)
    .bind(eliminated_minor)
    .bind(&statements)
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ConsolidationRunDto {
        id: run_public.to_string(),
        currency,
        as_of: as_of.to_string(),
        status: "completed".into(),
        eliminated_minor,
        entity_statements,
        consolidated_trial_balance,
        eliminations,
    }))
}

async fn list_runs(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
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
        perms::finance_consolidation_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<(String, String, NaiveDate, String, i64)> = sqlx::query_as(
        r#"
        SELECT public_id, currency, as_of, status, eliminated_minor
        FROM finance_consolidation_run WHERE org_id = $1 ORDER BY created_at DESC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, currency, as_of, status, eliminated_minor)| {
                json!({
                    "id": id,
                    "currency": currency,
                    "as_of": as_of.to_string(),
                    "status": status,
                    "eliminated_minor": eliminated_minor,
                })
            })
            .collect(),
    ))
}

async fn get_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ConsolidationRunDto>, AppError> {
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
        perms::finance_consolidation_read(),
        &request_id,
    )?;

    let run_id = parse_public_id(IdKind::ConsolidationRun, &id, &request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<(String, String, NaiveDate, String, i64, serde_json::Value)> = sqlx::query_as(
        r#"
        SELECT public_id, currency, as_of, status, eliminated_minor, statements
        FROM finance_consolidation_run WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(run_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    let Some((public_id, currency, as_of, status, eliminated_minor, statements)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "consolidation run not found",
        ));
    };

    let entity_statements: Vec<EntityStatementPack> =
        serde_json::from_value(statements["entity_statements"].clone()).unwrap_or_default();
    let consolidated_trial_balance: TrialBalanceResponse = serde_json::from_value(
        statements["consolidated_trial_balance"].clone(),
    )
    .unwrap_or(TrialBalanceResponse {
        currency: currency.clone(),
        period_id: None,
        rows: vec![],
        total_debit_minor: 0,
        total_credit_minor: 0,
        balanced: true,
    });
    let eliminations: Vec<EliminationLine> =
        serde_json::from_value(statements["eliminations"].clone()).unwrap_or_default();

    Ok(Json(ConsolidationRunDto {
        id: public_id,
        currency,
        as_of: as_of.to_string(),
        status,
        eliminated_minor,
        entity_statements,
        consolidated_trial_balance,
        eliminations,
    }))
}

/// Whether the caller may read journals for `entity_id` (None = unscoped).
pub fn can_read_entity_journals(
    principal: &companyos_authz::Principal,
    allowed_entities: &[Uuid],
    entity_id: Option<Uuid>,
) -> bool {
    if is_allowed(principal, &perms::finance_consolidation_run())
        || is_allowed(principal, &perms::finance_entity_manage())
    {
        return true;
    }
    match entity_id {
        None => allowed_entities.is_empty(), // unscoped journals: only if no entity ACL
        Some(eid) => allowed_entities.is_empty() || allowed_entities.contains(&eid),
    }
}
