//! Fiscal periods — open / closed / locked gating for journal posts.

use chrono::{Datelike, NaiveDate};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FiscalPeriodRow {
    pub id: Uuid,
    pub public_id: String,
    pub code: String,
    pub name: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub status: String,
}

/// Look up the period covering `entry_date`, creating an open monthly period if missing.
pub async fn ensure_period_for_date(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    entry_date: NaiveDate,
) -> Result<FiscalPeriodRow, sqlx::Error> {
    if let Some(row) = find_period_for_date(tx, org_id, entry_date).await? {
        return Ok(row);
    }
    let start = NaiveDate::from_ymd_opt(entry_date.year(), entry_date.month(), 1).expect("valid");
    let end = if entry_date.month() == 12 {
        NaiveDate::from_ymd_opt(entry_date.year() + 1, 1, 1)
            .expect("valid")
            .pred_opt()
            .expect("valid")
    } else {
        NaiveDate::from_ymd_opt(entry_date.year(), entry_date.month() + 1, 1)
            .expect("valid")
            .pred_opt()
            .expect("valid")
    };
    let code = format!("{:04}-{:02}", entry_date.year(), entry_date.month());
    let name = format!("{code} fiscal period");
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::FiscalPeriod, id).as_str();
    sqlx::query(
        r#"
        INSERT INTO finance_fiscal_period (
            id, org_id, public_id, code, name, start_date, end_date, status, checklist
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,'open','{}'::jsonb)
        ON CONFLICT (org_id, code) DO NOTHING
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(&public_id)
    .bind(&code)
    .bind(&name)
    .bind(start)
    .bind(end)
    .execute(&mut **tx)
    .await?;

    find_period_for_date(tx, org_id, entry_date)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)
}

pub async fn find_period_for_date(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    entry_date: NaiveDate,
) -> Result<Option<FiscalPeriodRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, public_id, code, name, start_date, end_date, status
        FROM finance_fiscal_period
        WHERE org_id = $1 AND start_date <= $2 AND end_date >= $2
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(entry_date)
    .fetch_optional(&mut **tx)
    .await
}

/// Reject posting into closed or locked periods with a clear RFC 9457 conflict.
pub fn assert_period_accepts_posting(
    period: &FiscalPeriodRow,
    request_id: &str,
) -> Result<(), AppError> {
    match period.status.as_str() {
        "open" => Ok(()),
        "closed" => Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            format!(
                "fiscal period {} ({}) is closed; reopen required before posting",
                period.code, period.public_id
            ),
        )),
        "locked" => Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            format!(
                "fiscal period {} ({}) is locked; reopen required before posting",
                period.code, period.public_id
            ),
        )),
        other => Err(AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("unknown period status: {other}"),
        )),
    }
}

/// Default month-end close checklist keys (boolean completion flags).
pub fn default_checklist() -> serde_json::Value {
    json!({
        "bank_reconciled": false,
        "expenses_posted": false,
        "accruals_reviewed": false,
        "trial_balance_reviewed": false,
        "payroll_posted": false
    })
}
