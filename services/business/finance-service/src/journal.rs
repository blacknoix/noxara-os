//! Chart-of-accounts seeding and journal posting.
//!
//! Posting rules (documented; finance reviewer has **not** signed off):
//! - Invoice issue: Dr AR (total), Cr Revenue (net), Cr Tax Payable (tax)
//! - Payment allocated: Dr Cash (allocated), Cr AR (allocated);
//!   overpayment: Dr Cash (unapplied), Cr Customer Credits
//! - Credit note: Dr Revenue (net), Dr Tax Payable (tax), Cr AR (total)
//! - Expense posted: Dr Expense category, Cr Cash
//!
//! Every entry must balance: sum(debit) == sum(credit).

use companyos_ids::new_uuid_v7;
use companyos_money::{Currency, Money, MoneyError};
use uuid::Uuid;

/// Standard ledger account codes seeded per org.
pub mod codes {
    pub const CASH: &str = "1000";
    pub const AR: &str = "1100";
    pub const CUSTOMER_CREDITS: &str = "2200";
    pub const TAX_PAYABLE: &str = "2100";
    pub const REVENUE: &str = "4000";
    pub const EXPENSE: &str = "5000";
    /// Wages / salary expense (payroll).
    pub const WAGES_EXPENSE: &str = "5100";
    /// Statutory & other payroll deductions payable.
    pub const PAYROLL_DEDUCTIONS: &str = "2300";
    /// Net pay clearing (ACH/CSV batch).
    pub const NET_PAY_CLEARING: &str = "2400";
    /// Inventory asset (Phase 2.5 — inventory-service receipts/issues).
    pub const INVENTORY: &str = "1200";
    /// Accounts payable — vendors (Phase 2.5 procure-to-pay).
    pub const AP_VENDORS: &str = "2000";
    /// Cost of goods sold (Phase 2.5 — stock issues).
    pub const COGS: &str = "5200";
    /// Depreciation expense (Phase 2.5 — fixed asset depreciation).
    pub const DEPRECIATION_EXPENSE: &str = "5300";
    /// Accumulated depreciation (contra-asset; Phase 2.5).
    pub const ACCUMULATED_DEPRECIATION: &str = "1300";
    /// Intercompany receivable (Phase 4.2).
    pub const IC_RECEIVABLE: &str = "1500";
    /// Intercompany payable (Phase 4.2).
    pub const IC_PAYABLE: &str = "2500";
    /// Intercompany revenue (Phase 4.2 eliminations).
    pub const IC_REVENUE: &str = "4900";
    /// Intercompany expense (Phase 4.2 eliminations).
    pub const IC_EXPENSE: &str = "5900";
}

#[derive(Debug, Clone)]
pub struct LedgerLine {
    pub account_code: String,
    pub debit_minor: i64,
    pub credit_minor: i64,
    pub memo: Option<String>,
}

impl LedgerLine {
    pub fn debit(account_code: impl Into<String>, amount: i64, memo: Option<String>) -> Self {
        Self {
            account_code: account_code.into(),
            debit_minor: amount,
            credit_minor: 0,
            memo,
        }
    }

    pub fn credit(account_code: impl Into<String>, amount: i64, memo: Option<String>) -> Self {
        Self {
            account_code: account_code.into(),
            debit_minor: 0,
            credit_minor: amount,
            memo,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JournalDraft {
    pub memo: String,
    pub source_type: &'static str,
    pub source_id: Uuid,
    pub currency: Currency,
    pub lines: Vec<LedgerLine>,
    /// Document date; defaults to today when unset by callers.
    pub entry_date: Option<chrono::NaiveDate>,
    /// Optional reversing link (immutable correction pattern).
    pub reverses_entry_id: Option<Uuid>,
    pub posted_by: Option<Uuid>,
    /// Optional finance entity stamp (multi-entity foundations).
    pub entity_id: Option<Uuid>,
}

impl JournalDraft {
    pub fn assert_balanced(&self) -> Result<(), MoneyError> {
        let mut debit: i64 = 0;
        let mut credit: i64 = 0;
        for l in &self.lines {
            debit = debit
                .checked_add(l.debit_minor)
                .ok_or(MoneyError::Overflow)?;
            credit = credit
                .checked_add(l.credit_minor)
                .ok_or(MoneyError::Overflow)?;
            if (l.debit_minor > 0 && l.credit_minor > 0)
                || (l.debit_minor == 0 && l.credit_minor == 0)
            {
                return Err(MoneyError::Overflow);
            }
        }
        if debit != credit {
            return Err(MoneyError::Overflow);
        }
        // Touch Money to keep currency invariant visible for callers.
        let _ = Money::new(debit, self.currency);
        Ok(())
    }
}

pub fn invoice_issue_entry(
    invoice_id: Uuid,
    currency: Currency,
    net_minor: i64,
    tax_minor: i64,
    total_minor: i64,
) -> Result<JournalDraft, MoneyError> {
    let draft = JournalDraft {
        memo: format!("Invoice issue {invoice_id}"),
        source_type: "invoice_issue",
        source_id: invoice_id,
        currency,
        entry_date: None,
        reverses_entry_id: None,
        posted_by: None,
        entity_id: None,
        lines: vec![
            LedgerLine::debit(codes::AR, total_minor, Some("Accounts receivable".into())),
            LedgerLine::credit(codes::REVENUE, net_minor, Some("Revenue".into())),
            LedgerLine::credit(codes::TAX_PAYABLE, tax_minor, Some("Tax payable".into())),
        ]
        .into_iter()
        .filter(|l| l.debit_minor > 0 || l.credit_minor > 0)
        .collect(),
    };
    draft.assert_balanced()?;
    Ok(draft)
}

pub fn payment_entry(
    payment_id: Uuid,
    currency: Currency,
    allocated_minor: i64,
    unapplied_minor: i64,
) -> Result<JournalDraft, MoneyError> {
    let cash = allocated_minor
        .checked_add(unapplied_minor)
        .ok_or(MoneyError::Overflow)?;
    let mut lines = vec![LedgerLine::debit(
        codes::CASH,
        cash,
        Some("Cash received".into()),
    )];
    if allocated_minor > 0 {
        lines.push(LedgerLine::credit(
            codes::AR,
            allocated_minor,
            Some("AR settlement".into()),
        ));
    }
    if unapplied_minor > 0 {
        lines.push(LedgerLine::credit(
            codes::CUSTOMER_CREDITS,
            unapplied_minor,
            Some("Customer credit / overpayment".into()),
        ));
    }
    let draft = JournalDraft {
        memo: format!("Payment {payment_id}"),
        source_type: "payment",
        source_id: payment_id,
        currency,
        entry_date: None,
        reverses_entry_id: None,
        posted_by: None,
        entity_id: None,
        lines,
    };
    draft.assert_balanced()?;
    Ok(draft)
}

pub fn credit_note_entry(
    credit_id: Uuid,
    currency: Currency,
    net_minor: i64,
    tax_minor: i64,
    total_minor: i64,
) -> Result<JournalDraft, MoneyError> {
    let mut lines = Vec::new();
    if net_minor > 0 {
        lines.push(LedgerLine::debit(
            codes::REVENUE,
            net_minor,
            Some("Revenue reversal".into()),
        ));
    }
    if tax_minor > 0 {
        lines.push(LedgerLine::debit(
            codes::TAX_PAYABLE,
            tax_minor,
            Some("Tax reversal".into()),
        ));
    }
    lines.push(LedgerLine::credit(
        codes::AR,
        total_minor,
        Some("AR credit".into()),
    ));
    let draft = JournalDraft {
        memo: format!("Credit note {credit_id}"),
        source_type: "credit_note",
        source_id: credit_id,
        currency,
        entry_date: None,
        reverses_entry_id: None,
        posted_by: None,
        entity_id: None,
        lines,
    };
    draft.assert_balanced()?;
    Ok(draft)
}

pub fn expense_entry(
    expense_id: Uuid,
    currency: Currency,
    amount_minor: i64,
) -> Result<JournalDraft, MoneyError> {
    let draft = JournalDraft {
        memo: format!("Expense {expense_id}"),
        source_type: "expense",
        source_id: expense_id,
        currency,
        entry_date: None,
        reverses_entry_id: None,
        posted_by: None,
        entity_id: None,
        lines: vec![
            LedgerLine::debit(codes::EXPENSE, amount_minor, Some("Expense".into())),
            LedgerLine::credit(codes::CASH, amount_minor, Some("Cash".into())),
        ],
    };
    draft.assert_balanced()?;
    Ok(draft)
}

/// Payroll run posting: Dr Wages, Cr Deductions Payable, Cr Net Pay Clearing.
pub fn payroll_entry(
    run_id: Uuid,
    currency: Currency,
    gross_minor: i64,
    deductions_minor: i64,
    net_minor: i64,
) -> Result<JournalDraft, MoneyError> {
    if gross_minor
        .checked_sub(deductions_minor)
        .ok_or(MoneyError::Overflow)?
        != net_minor
    {
        return Err(MoneyError::Overflow);
    }
    let mut lines = vec![LedgerLine::debit(
        codes::WAGES_EXPENSE,
        gross_minor,
        Some("Wages expense".into()),
    )];
    if deductions_minor > 0 {
        lines.push(LedgerLine::credit(
            codes::PAYROLL_DEDUCTIONS,
            deductions_minor,
            Some("Payroll deductions payable".into()),
        ));
    }
    if net_minor > 0 {
        lines.push(LedgerLine::credit(
            codes::NET_PAY_CLEARING,
            net_minor,
            Some("Net pay clearing".into()),
        ));
    }
    let draft = JournalDraft {
        memo: format!("Payroll run {run_id}"),
        source_type: "payroll",
        source_id: run_id,
        currency,
        entry_date: None,
        reverses_entry_id: None,
        posted_by: None,
        entity_id: None,
        lines,
    };
    draft.assert_balanced()?;
    Ok(draft)
}

/// Ensure standard ledger accounts exist for `org_id` (idempotent).
pub async fn ensure_ledger_accounts(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<(), sqlx::Error> {
    use companyos_ids::{IdKind, PublicId};

    // (code, name, type, normal, sort)
    let accounts: &[(&str, &str, &str, &str, i32)] = &[
        (codes::CASH, "Cash", "asset", "debit", 100),
        (codes::AR, "Accounts Receivable", "asset", "debit", 110),
        (
            codes::TAX_PAYABLE,
            "Tax Payable",
            "liability",
            "credit",
            210,
        ),
        (
            codes::CUSTOMER_CREDITS,
            "Customer Credits",
            "liability",
            "credit",
            220,
        ),
        (
            codes::PAYROLL_DEDUCTIONS,
            "Payroll Deductions Payable",
            "liability",
            "credit",
            230,
        ),
        (
            codes::NET_PAY_CLEARING,
            "Net Pay Clearing",
            "liability",
            "credit",
            240,
        ),
        ("3000", "Retained Earnings", "equity", "credit", 300),
        (codes::REVENUE, "Revenue", "revenue", "credit", 400),
        (
            codes::EXPENSE,
            "Operating Expenses",
            "expense",
            "debit",
            500,
        ),
        (
            codes::WAGES_EXPENSE,
            "Wages Expense",
            "expense",
            "debit",
            510,
        ),
        (codes::INVENTORY, "Inventory", "asset", "debit", 120),
        (
            codes::AP_VENDORS,
            "Accounts Payable — Vendors",
            "liability",
            "credit",
            200,
        ),
        (codes::COGS, "Cost of Goods Sold", "expense", "debit", 520),
        (
            codes::DEPRECIATION_EXPENSE,
            "Depreciation Expense",
            "expense",
            "debit",
            530,
        ),
        (
            codes::ACCUMULATED_DEPRECIATION,
            "Accumulated Depreciation",
            "asset",
            "credit",
            130,
        ),
        (
            codes::IC_RECEIVABLE,
            "Intercompany Receivable",
            "asset",
            "debit",
            150,
        ),
        (
            codes::IC_PAYABLE,
            "Intercompany Payable",
            "liability",
            "credit",
            250,
        ),
        (
            codes::IC_REVENUE,
            "Intercompany Revenue",
            "revenue",
            "credit",
            490,
        ),
        (
            codes::IC_EXPENSE,
            "Intercompany Expense",
            "expense",
            "debit",
            590,
        ),
    ];
    for (code, name, ty, normal, sort) in accounts {
        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::LedgerAccount, id).as_str();
        sqlx::query(
            r#"
            INSERT INTO finance_ledger_account (
                id, org_id, public_id, code, name, account_type, normal_balance, sort_order, is_active
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)
            ON CONFLICT (org_id, code) DO UPDATE
                SET public_id = COALESCE(finance_ledger_account.public_id, EXCLUDED.public_id),
                    name = EXCLUDED.name,
                    sort_order = EXCLUDED.sort_order
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(&public_id)
        .bind(code)
        .bind(name)
        .bind(ty)
        .bind(normal)
        .bind(sort)
        .execute(&mut **tx)
        .await?;
    }
    // Expense categories used by policy / mileage / per-diem.
    for (code, name) in [
        ("general", "General"),
        ("travel", "Travel"),
        ("meals", "Meals"),
        ("mileage", "Mileage"),
        ("per_diem", "Per Diem"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO finance_expense_category (id, org_id, code, name)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (org_id, code) DO NOTHING
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(code)
        .bind(name)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub async fn account_id_by_code(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    code: &str,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM finance_ledger_account WHERE org_id = $1 AND code = $2")
            .bind(org_id)
            .bind(code)
            .fetch_one(&mut **tx)
            .await?;
    Ok(id)
}

/// Persist a balanced journal draft inside the caller's transaction.
///
/// Enforces: balance, open fiscal period, active account codes.
pub async fn post_journal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    draft: &JournalDraft,
    request_id: &str,
) -> Result<Uuid, companyos_errors::AppError> {
    use crate::periods::{assert_period_accepts_posting, ensure_period_for_date};
    use companyos_errors::{AppError, ErrorCode};

    draft.assert_balanced().map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "journal lines must balance",
        )
    })?;

    let entry_date = draft
        .entry_date
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    let period = ensure_period_for_date(tx, org_id, entry_date)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;
    assert_period_accepts_posting(&period, request_id)?;

    let entry_id = new_uuid_v7();
    let public_id = format!("jrn_{entry_id}");
    sqlx::query(
        r#"
        INSERT INTO finance_journal_entry (
            id, org_id, public_id, entry_date, memo, source_type, source_id, currency,
            period_id, reverses_entry_id, posted_by, entity_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        "#,
    )
    .bind(entry_id)
    .bind(org_id)
    .bind(&public_id)
    .bind(entry_date)
    .bind(&draft.memo)
    .bind(draft.source_type)
    .bind(draft.source_id)
    .bind(draft.currency.as_str())
    .bind(period.id)
    .bind(draft.reverses_entry_id)
    .bind(draft.posted_by)
    .bind(draft.entity_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;

    for line in &draft.lines {
        let acct = match account_id_by_code(tx, org_id, &line.account_code).await {
            Ok(id) => id,
            Err(_) => {
                return Err(AppError::new(
                    ErrorCode::ValidationFailed,
                    request_id,
                    format!("unknown account_code: {}", line.account_code),
                ));
            }
        };
        let active: bool = sqlx::query_scalar(
            "SELECT is_active FROM finance_ledger_account WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(acct)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;
        if !active {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                format!("account {} is inactive", line.account_code),
            ));
        }
        sqlx::query(
            r#"
            INSERT INTO finance_journal_line (
                id, org_id, entry_id, account_id, debit_minor, credit_minor, memo
            ) VALUES ($1,$2,$3,$4,$5,$6,$7)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(entry_id)
        .bind(acct)
        .bind(line.debit_minor)
        .bind(line.credit_minor)
        .bind(line.memo.as_deref())
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("db error: {e}")))?;
    }
    Ok(entry_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn invoice_issue_balances() {
        let d = invoice_issue_entry(Uuid::nil(), Currency::USD, 9000, 1000, 10_000).unwrap();
        d.assert_balanced().unwrap();
    }

    #[test]
    fn payment_with_overpayment_balances() {
        let d = payment_entry(Uuid::nil(), Currency::USD, 8000, 2000).unwrap();
        d.assert_balanced().unwrap();
    }

    #[test]
    fn credit_note_balances() {
        let d = credit_note_entry(Uuid::nil(), Currency::EUR, 4500, 500, 5000).unwrap();
        d.assert_balanced().unwrap();
    }

    #[test]
    fn expense_balances() {
        let d = expense_entry(Uuid::nil(), Currency::USD, 12345).unwrap();
        d.assert_balanced().unwrap();
    }

    #[test]
    fn payroll_balances() {
        let d = payroll_entry(Uuid::nil(), Currency::USD, 10_000, 2_000, 8_000).unwrap();
        d.assert_balanced().unwrap();
        assert_eq!(d.source_type, "payroll");
    }

    #[test]
    fn payroll_rejects_unbalanced_net() {
        assert!(payroll_entry(Uuid::nil(), Currency::USD, 10_000, 2_000, 7_000).is_err());
    }

    #[test]
    fn zero_tax_invoice_ok() {
        let d = invoice_issue_entry(Uuid::nil(), Currency::USD, 1000, 0, 1000).unwrap();
        assert_eq!(d.lines.len(), 2);
        d.assert_balanced().unwrap();
    }

    proptest! {
        #[test]
        fn prop_invoice_journal_balanced(
            net in 1i64..1_000_000,
            tax in 0i64..100_000,
        ) {
            let total = net + tax;
            let d = invoice_issue_entry(Uuid::nil(), Currency::USD, net, tax, total).unwrap();
            let debit: i64 = d.lines.iter().map(|l| l.debit_minor).sum();
            let credit: i64 = d.lines.iter().map(|l| l.credit_minor).sum();
            prop_assert_eq!(debit, credit);
            prop_assert_eq!(debit, total);
        }

        #[test]
        fn prop_payment_journal_balanced(
            allocated in 0i64..1_000_000,
            unapplied in 0i64..1_000_000,
        ) {
            prop_assume!(allocated + unapplied > 0);
            let d = payment_entry(Uuid::nil(), Currency::USD, allocated, unapplied).unwrap();
            let debit: i64 = d.lines.iter().map(|l| l.debit_minor).sum();
            let credit: i64 = d.lines.iter().map(|l| l.credit_minor).sum();
            prop_assert_eq!(debit, credit);
        }
    }
}
