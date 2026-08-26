//! `/api/v1/finance/reports/summary` — revenue, expenses, cash, AR ageing.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;

use super::internal;
use crate::auth::AuthCtx;
use crate::journal::codes;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{AgeingBucket, CashFlowPoint, CategoryAmount, ReportSummaryDto};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/finance/reports/summary", get(report_summary))
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

    // Revenue: sum of issued (non-void) invoice totals in document currency (prefer USD base).
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

    // Cash / AR from journal balances (debit − credit for asset accounts).
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
