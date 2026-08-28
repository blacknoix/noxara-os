//! Finance reports — summary, trial balance, P&L, balance sheet.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::IdKind;
use companyos_tenancy::set_session_org_id;
use serde::Deserialize;
use uuid::Uuid;

use super::{internal, parse_public_id, validation};
use crate::auth::AuthCtx;
use crate::journal::codes;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    AgeingBucket, BalanceSheetResponse, CashFlowPoint, CategoryAmount, ProfitAndLossResponse,
    ReportLine, ReportSummaryDto, TrialBalanceResponse, TrialBalanceRow,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/finance/reports/summary", get(report_summary))
        .route("/api/v1/finance/reports/trial-balance", get(trial_balance))
        .route(
            "/api/v1/finance/reports/profit-and-loss",
            get(profit_and_loss),
        )
        .route("/api/v1/finance/reports/balance-sheet", get(balance_sheet))
}

#[derive(Debug, Deserialize)]
pub struct TrialBalanceQuery {
    pub period_id: Option<String>,
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProfitAndLossQuery {
    pub period_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BalanceSheetQuery {
    pub period_id: Option<String>,
    pub as_of: Option<String>,
    pub currency: Option<String>,
}

fn parse_date(request_id: &str, raw: &str, field: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| validation(request_id, format!("{field} must be YYYY-MM-DD")))
}

/// GET /api/v1/finance/reports/summary
#[utoipa::path(get, path = "/api/v1/finance/reports/summary", tag = "finance-reports",
    responses((status = 200, body = ReportSummaryDto)))]
pub async fn report_summary(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ReportSummaryDto>, AppError> {
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
        perms::finance_report_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let revenue_minor: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(base_total_minor), 0)::BIGINT
        FROM finance_invoice
        WHERE org_id = $1 AND status NOT IN ('draft', 'void')
        "#,
    )
    .bind(org_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let expenses_minor: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(amount_minor), 0)::BIGINT
        FROM finance_expense
        WHERE org_id = $1 AND status IN ('approved', 'posted')
        "#,
    )
    .bind(org_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let cash_minor: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(jl.debit_minor - jl.credit_minor), 0)::BIGINT
        FROM finance_journal_line jl
        JOIN finance_ledger_account a ON a.id = jl.account_id
        WHERE jl.org_id = $1 AND a.code = $2
        "#,
    )
    .bind(org_id)
    .bind(codes::CASH)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let receivables_minor: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(balance_minor), 0)::BIGINT
        FROM finance_invoice
        WHERE org_id = $1 AND status NOT IN ('draft', 'void', 'paid')
        "#,
    )
    .bind(org_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    #[derive(sqlx::FromRow)]
    struct AgeRow {
        label: String,
        amount_minor: i64,
    }
    let age_rows: Vec<AgeRow> = sqlx::query_as(
        r#"
        SELECT bucket AS label, COALESCE(SUM(balance_minor), 0)::BIGINT AS amount_minor
        FROM (
            SELECT balance_minor,
                   CASE
                     WHEN due_date IS NULL OR due_date >= CURRENT_DATE THEN 'current'
                     WHEN CURRENT_DATE - due_date <= 30 THEN '1_30'
                     WHEN CURRENT_DATE - due_date <= 60 THEN '31_60'
                     WHEN CURRENT_DATE - due_date <= 90 THEN '61_90'
                     ELSE '90_plus'
                   END AS bucket
            FROM finance_invoice
            WHERE org_id = $1 AND status NOT IN ('draft', 'void', 'paid')
              AND balance_minor > 0
        ) t
        GROUP BY bucket
        ORDER BY bucket
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let ageing: Vec<AgeingBucket> = age_rows
        .into_iter()
        .map(|r| AgeingBucket {
            label: r.label,
            amount_minor: r.amount_minor,
        })
        .collect();

    #[derive(sqlx::FromRow)]
    struct CatRow {
        category: String,
        amount_minor: i64,
    }
    let cat_rows: Vec<CatRow> = sqlx::query_as(
        r#"
        SELECT COALESCE(c.code, 'uncategorized') AS category,
               COALESCE(SUM(e.amount_minor), 0)::BIGINT AS amount_minor
        FROM finance_expense e
        LEFT JOIN finance_expense_category c ON c.id = e.category_id
        WHERE e.org_id = $1 AND e.status IN ('approved', 'posted')
        GROUP BY c.code
        ORDER BY amount_minor DESC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let expenses_by_category: Vec<CategoryAmount> = cat_rows
        .into_iter()
        .map(|r| CategoryAmount {
            category: r.category,
            amount_minor: r.amount_minor,
        })
        .collect();

    #[derive(sqlx::FromRow)]
    struct CfRow {
        period: String,
        inflow_minor: i64,
        outflow_minor: i64,
    }
    let cf_rows: Vec<CfRow> = sqlx::query_as(
        r#"
        SELECT to_char(je.entry_date, 'YYYY-MM') AS period,
               COALESCE(SUM(CASE WHEN a.code = $2 THEN jl.debit_minor ELSE 0 END), 0)::BIGINT
                   AS inflow_minor,
               COALESCE(SUM(CASE WHEN a.code = $2 THEN jl.credit_minor ELSE 0 END), 0)::BIGINT
                   AS outflow_minor
        FROM finance_journal_entry je
        JOIN finance_journal_line jl ON jl.entry_id = je.id
        JOIN finance_ledger_account a ON a.id = jl.account_id
        WHERE je.org_id = $1
        GROUP BY to_char(je.entry_date, 'YYYY-MM')
        ORDER BY period DESC
        LIMIT 12
        "#,
    )
    .bind(org_id)
    .bind(codes::CASH)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let cash_flow: Vec<CashFlowPoint> = cf_rows
        .into_iter()
        .map(|r| CashFlowPoint {
            period: r.period,
            inflow_minor: r.inflow_minor,
            outflow_minor: r.outflow_minor,
        })
        .collect();

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ReportSummaryDto {
        as_of: Utc::now().to_rfc3339(),
        currency: "USD".into(),
        revenue_minor,
        expenses_minor,
        cash_minor,
        receivables_minor,
        ageing,
        expenses_by_category,
        cash_flow,
    }))
}

/// GET /api/v1/finance/reports/trial-balance
#[utoipa::path(
    get,
    path = "/api/v1/finance/reports/trial-balance",
    tag = "finance-reports",
    params(
        ("period_id" = Option<String>, Query),
        ("currency" = Option<String>, Query),
    ),
    responses((status = 200, body = TrialBalanceResponse))
)]
pub async fn trial_balance(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<TrialBalanceQuery>,
) -> Result<Json<TrialBalanceResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let currency = q.currency.unwrap_or_else(|| "USD".into());

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_report_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let (period_uuid, period_public): (Option<Uuid>, Option<String>) =
        if let Some(ref pid) = q.period_id {
            let id = parse_public_id(IdKind::FiscalPeriod, pid, &request_id)?;
            let pub_id: Option<String> = sqlx::query_scalar(
                "SELECT public_id FROM finance_fiscal_period WHERE org_id = $1 AND id = $2",
            )
            .bind(org_id)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            if pub_id.is_none() {
                return Err(validation(&request_id, "period not found"));
            }
            (Some(id), pub_id)
        } else {
            (None, None)
        };

    #[derive(sqlx::FromRow)]
    struct TbRow {
        account_code: String,
        account_name: String,
        account_type: String,
        debit_minor: i64,
        credit_minor: i64,
    }

    let rows: Vec<TbRow> = if let Some(pid) = period_uuid {
        sqlx::query_as(
            r#"
            SELECT a.code AS account_code, a.name AS account_name, a.account_type,
                   COALESCE(SUM(jl.debit_minor), 0)::BIGINT AS debit_minor,
                   COALESCE(SUM(jl.credit_minor), 0)::BIGINT AS credit_minor
            FROM finance_journal_line jl
            JOIN finance_journal_entry je ON je.id = jl.entry_id
            JOIN finance_ledger_account a ON a.id = jl.account_id
            WHERE jl.org_id = $1 AND je.currency = $2 AND je.period_id = $3
            GROUP BY a.code, a.name, a.account_type
            HAVING COALESCE(SUM(jl.debit_minor), 0) <> 0
                OR COALESCE(SUM(jl.credit_minor), 0) <> 0
            ORDER BY a.code
            "#,
        )
        .bind(org_id)
        .bind(&currency)
        .bind(pid)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?
    } else {
        sqlx::query_as(
            r#"
            SELECT a.code AS account_code, a.name AS account_name, a.account_type,
                   COALESCE(SUM(jl.debit_minor), 0)::BIGINT AS debit_minor,
                   COALESCE(SUM(jl.credit_minor), 0)::BIGINT AS credit_minor
            FROM finance_journal_line jl
            JOIN finance_journal_entry je ON je.id = jl.entry_id
            JOIN finance_ledger_account a ON a.id = jl.account_id
            WHERE jl.org_id = $1 AND je.currency = $2
            GROUP BY a.code, a.name, a.account_type
            HAVING COALESCE(SUM(jl.debit_minor), 0) <> 0
                OR COALESCE(SUM(jl.credit_minor), 0) <> 0
            ORDER BY a.code
            "#,
        )
        .bind(org_id)
        .bind(&currency)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?
    };

    tx.commit().await.map_err(internal(&request_id))?;

    let total_debit_minor: i64 = rows.iter().map(|r| r.debit_minor).sum();
    let total_credit_minor: i64 = rows.iter().map(|r| r.credit_minor).sum();
    let balanced = total_debit_minor == total_credit_minor;

    Ok(Json(TrialBalanceResponse {
        currency,
        period_id: period_public,
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
        balanced,
    }))
}

/// GET /api/v1/finance/reports/profit-and-loss
#[utoipa::path(
    get,
    path = "/api/v1/finance/reports/profit-and-loss",
    tag = "finance-reports",
    params(
        ("period_id" = Option<String>, Query),
        ("from" = Option<String>, Query),
        ("to" = Option<String>, Query),
        ("currency" = Option<String>, Query),
    ),
    responses((status = 200, body = ProfitAndLossResponse))
)]
pub async fn profit_and_loss(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ProfitAndLossQuery>,
) -> Result<Json<ProfitAndLossResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let currency = q.currency.unwrap_or_else(|| "USD".into());

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_report_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut from_date: Option<NaiveDate> = None;
    let mut to_date: Option<NaiveDate> = None;
    let mut period_public: Option<String> = None;

    if let Some(ref pid) = q.period_id {
        let id = parse_public_id(IdKind::FiscalPeriod, pid, &request_id)?;
        let row: Option<(String, NaiveDate, NaiveDate)> = sqlx::query_as(
            "SELECT public_id, start_date, end_date FROM finance_fiscal_period WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        let Some((pub_id, start, end)) = row else {
            return Err(validation(&request_id, "period not found"));
        };
        period_public = Some(pub_id);
        from_date = Some(start);
        to_date = Some(end);
    } else {
        if let Some(ref f) = q.from {
            from_date = Some(parse_date(&request_id, f, "from")?);
        }
        if let Some(ref t) = q.to {
            to_date = Some(parse_date(&request_id, t, "to")?);
        }
    }

    #[derive(sqlx::FromRow)]
    struct LineRow {
        account_code: String,
        account_name: String,
        account_type: String,
        amount_minor: i64,
    }

    // Revenue/income: credit − debit; expense: debit − credit (normal balances).
    let rows: Vec<LineRow> = sqlx::query_as(
        r#"
        SELECT a.code AS account_code, a.name AS account_name, a.account_type,
               CASE
                 WHEN a.account_type IN ('revenue', 'income')
                   THEN COALESCE(SUM(jl.credit_minor - jl.debit_minor), 0)::BIGINT
                 ELSE COALESCE(SUM(jl.debit_minor - jl.credit_minor), 0)::BIGINT
               END AS amount_minor
        FROM finance_journal_line jl
        JOIN finance_journal_entry je ON je.id = jl.entry_id
        JOIN finance_ledger_account a ON a.id = jl.account_id
        WHERE jl.org_id = $1
          AND je.currency = $2
          AND a.account_type IN ('revenue', 'income', 'expense')
          AND ($3::date IS NULL OR je.entry_date >= $3)
          AND ($4::date IS NULL OR je.entry_date <= $4)
        GROUP BY a.code, a.name, a.account_type
        HAVING CASE
                 WHEN a.account_type IN ('revenue', 'income')
                   THEN COALESCE(SUM(jl.credit_minor - jl.debit_minor), 0)
                 ELSE COALESCE(SUM(jl.debit_minor - jl.credit_minor), 0)
               END <> 0
        ORDER BY a.code
        "#,
    )
    .bind(org_id)
    .bind(&currency)
    .bind(from_date)
    .bind(to_date)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    let mut revenue = Vec::new();
    let mut expenses = Vec::new();
    for r in rows {
        let line = ReportLine {
            account_code: r.account_code,
            account_name: r.account_name,
            amount_minor: r.amount_minor,
        };
        if r.account_type == "expense" {
            expenses.push(line);
        } else {
            revenue.push(line);
        }
    }
    let revenue_total_minor: i64 = revenue.iter().map(|l| l.amount_minor).sum();
    let expense_total_minor: i64 = expenses.iter().map(|l| l.amount_minor).sum();

    Ok(Json(ProfitAndLossResponse {
        currency,
        from: from_date.map(|d| d.to_string()),
        to: to_date.map(|d| d.to_string()),
        period_id: period_public,
        revenue,
        expenses,
        revenue_total_minor,
        expense_total_minor,
        net_income_minor: revenue_total_minor - expense_total_minor,
    }))
}

/// GET /api/v1/finance/reports/balance-sheet
#[utoipa::path(
    get,
    path = "/api/v1/finance/reports/balance-sheet",
    tag = "finance-reports",
    params(
        ("period_id" = Option<String>, Query),
        ("as_of" = Option<String>, Query),
        ("currency" = Option<String>, Query),
    ),
    responses((status = 200, body = BalanceSheetResponse))
)]
pub async fn balance_sheet(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<BalanceSheetQuery>,
) -> Result<Json<BalanceSheetResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let currency = q.currency.unwrap_or_else(|| "USD".into());

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_report_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut as_of = Utc::now().date_naive();
    let mut period_public: Option<String> = None;

    if let Some(ref pid) = q.period_id {
        let id = parse_public_id(IdKind::FiscalPeriod, pid, &request_id)?;
        let row: Option<(String, NaiveDate)> = sqlx::query_as(
            "SELECT public_id, end_date FROM finance_fiscal_period WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        let Some((pub_id, end)) = row else {
            return Err(validation(&request_id, "period not found"));
        };
        period_public = Some(pub_id);
        as_of = end;
    } else if let Some(ref d) = q.as_of {
        as_of = parse_date(&request_id, d, "as_of")?;
    }

    #[derive(sqlx::FromRow)]
    struct LineRow {
        account_code: String,
        account_name: String,
        account_type: String,
        amount_minor: i64,
    }

    let rows: Vec<LineRow> = sqlx::query_as(
        r#"
        SELECT a.code AS account_code, a.name AS account_name, a.account_type,
               CASE
                 WHEN a.account_type = 'asset'
                   THEN COALESCE(SUM(jl.debit_minor - jl.credit_minor), 0)::BIGINT
                 ELSE COALESCE(SUM(jl.credit_minor - jl.debit_minor), 0)::BIGINT
               END AS amount_minor
        FROM finance_journal_line jl
        JOIN finance_journal_entry je ON je.id = jl.entry_id
        JOIN finance_ledger_account a ON a.id = jl.account_id
        WHERE jl.org_id = $1
          AND je.currency = $2
          AND je.entry_date <= $3
          AND a.account_type IN ('asset', 'liability', 'equity')
        GROUP BY a.code, a.name, a.account_type
        HAVING CASE
                 WHEN a.account_type = 'asset'
                   THEN COALESCE(SUM(jl.debit_minor - jl.credit_minor), 0)
                 ELSE COALESCE(SUM(jl.credit_minor - jl.debit_minor), 0)
               END <> 0
        ORDER BY a.code
        "#,
    )
    .bind(org_id)
    .bind(&currency)
    .bind(as_of)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    let mut assets = Vec::new();
    let mut liabilities = Vec::new();
    let mut equity = Vec::new();
    for r in rows {
        let line = ReportLine {
            account_code: r.account_code,
            account_name: r.account_name,
            amount_minor: r.amount_minor,
        };
        match r.account_type.as_str() {
            "asset" => assets.push(line),
            "liability" => liabilities.push(line),
            _ => equity.push(line),
        }
    }
    let assets_total_minor: i64 = assets.iter().map(|l| l.amount_minor).sum();
    let liabilities_total_minor: i64 = liabilities.iter().map(|l| l.amount_minor).sum();
    let equity_total_minor: i64 = equity.iter().map(|l| l.amount_minor).sum();

    Ok(Json(BalanceSheetResponse {
        currency,
        as_of: as_of.to_string(),
        period_id: period_public,
        assets,
        liabilities,
        equity,
        assets_total_minor,
        liabilities_total_minor,
        equity_total_minor,
    }))
}
