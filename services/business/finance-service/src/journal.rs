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
}

#[derive(Debug, Clone)]
pub struct LedgerLine {
    pub account_code: &'static str,
    pub debit_minor: i64,
    pub credit_minor: i64,
    pub memo: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JournalDraft {
    pub memo: String,
    pub source_type: &'static str,
    pub source_id: Uuid,
    pub currency: Currency,
    pub lines: Vec<LedgerLine>,
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
        lines: vec![
            LedgerLine {
                account_code: codes::AR,
                debit_minor: total_minor,
                credit_minor: 0,
                memo: Some("Accounts receivable".into()),
            },
            LedgerLine {
                account_code: codes::REVENUE,
                debit_minor: 0,
                credit_minor: net_minor,
                memo: Some("Revenue".into()),
            },
            LedgerLine {
                account_code: codes::TAX_PAYABLE,
                debit_minor: 0,
                credit_minor: tax_minor,
                memo: Some("Tax payable".into()),
            },
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
    let mut lines = vec![LedgerLine {
        account_code: codes::CASH,
        debit_minor: cash,
        credit_minor: 0,
        memo: Some("Cash received".into()),
    }];
    if allocated_minor > 0 {
        lines.push(LedgerLine {
            account_code: codes::AR,
            debit_minor: 0,
            credit_minor: allocated_minor,
            memo: Some("AR settlement".into()),
        });
    }
    if unapplied_minor > 0 {
        lines.push(LedgerLine {
            account_code: codes::CUSTOMER_CREDITS,
            debit_minor: 0,
            credit_minor: unapplied_minor,
            memo: Some("Customer credit / overpayment".into()),
        });
    }
    let draft = JournalDraft {
        memo: format!("Payment {payment_id}"),
        source_type: "payment",
        source_id: payment_id,
        currency,
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
        lines.push(LedgerLine {
            account_code: codes::REVENUE,
            debit_minor: net_minor,
            credit_minor: 0,
            memo: Some("Revenue reversal".into()),
        });
    }
    if tax_minor > 0 {
        lines.push(LedgerLine {
            account_code: codes::TAX_PAYABLE,
            debit_minor: tax_minor,
            credit_minor: 0,
            memo: Some("Tax reversal".into()),
        });
    }
    lines.push(LedgerLine {
        account_code: codes::AR,
        debit_minor: 0,
        credit_minor: total_minor,
        memo: Some("AR credit".into()),
    });
    let draft = JournalDraft {
        memo: format!("Credit note {credit_id}"),
        source_type: "credit_note",
        source_id: credit_id,
        currency,
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
        lines: vec![
            LedgerLine {
                account_code: codes::EXPENSE,
                debit_minor: amount_minor,
                credit_minor: 0,
                memo: Some("Expense".into()),
            },
            LedgerLine {
                account_code: codes::CASH,
                debit_minor: 0,
                credit_minor: amount_minor,
                memo: Some("Cash".into()),
            },
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
    let mut lines = vec![LedgerLine {
        account_code: codes::WAGES_EXPENSE,
        debit_minor: gross_minor,
        credit_minor: 0,
        memo: Some("Wages expense".into()),
    }];
    if deductions_minor > 0 {
        lines.push(LedgerLine {
            account_code: codes::PAYROLL_DEDUCTIONS,
            debit_minor: 0,
            credit_minor: deductions_minor,
            memo: Some("Payroll deductions payable".into()),
        });
    }
    if net_minor > 0 {
        lines.push(LedgerLine {
            account_code: codes::NET_PAY_CLEARING,
            debit_minor: 0,
            credit_minor: net_minor,
            memo: Some("Net pay clearing".into()),
        });
    }
    let draft = JournalDraft {
        memo: format!("Payroll run {run_id}"),
        source_type: "payroll",
        source_id: run_id,
        currency,
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
    let accounts: &[(&str, &str, &str, &str)] = &[
        (codes::CASH, "Cash", "asset", "debit"),
        (codes::AR, "Accounts Receivable", "asset", "debit"),
        (codes::TAX_PAYABLE, "Tax Payable", "liability", "credit"),
        (
            codes::CUSTOMER_CREDITS,
            "Customer Credits",
            "liability",
            "credit",
        ),
        (codes::REVENUE, "Revenue", "revenue", "credit"),
        (codes::EXPENSE, "Operating Expenses", "expense", "debit"),
        (codes::WAGES_EXPENSE, "Wages Expense", "expense", "debit"),
        (
            codes::PAYROLL_DEDUCTIONS,
            "Payroll Deductions Payable",
            "liability",
            "credit",
        ),
        (
            codes::NET_PAY_CLEARING,
            "Net Pay Clearing",
            "liability",
            "credit",
        ),
    ];
    for (code, name, ty, normal) in accounts {
        sqlx::query(
            r#"
            INSERT INTO finance_ledger_account (id, org_id, code, name, account_type, normal_balance)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (org_id, code) DO NOTHING
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(code)
        .bind(name)
        .bind(ty)
        .bind(normal)
        .execute(&mut **tx)
        .await?;
    }
    // Default expense category
    sqlx::query(
        r#"
        INSERT INTO finance_expense_category (id, org_id, code, name)
        VALUES ($1, $2, 'general', 'General')
        ON CONFLICT (org_id, code) DO NOTHING
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id)
    .execute(&mut **tx)
    .await?;
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
pub async fn post_journal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    draft: &JournalDraft,
) -> Result<Uuid, sqlx::Error> {
    draft
        .assert_balanced()
        .map_err(|e| sqlx::Error::Protocol(format!("unbalanced journal: {e}")))?;
    let entry_id = new_uuid_v7();
    let public_id = format!("jrn_{entry_id}");
    sqlx::query(
        r#"
        INSERT INTO finance_journal_entry (
            id, org_id, public_id, memo, source_type, source_id, currency
        ) VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(entry_id)
    .bind(org_id)
    .bind(&public_id)
    .bind(&draft.memo)
    .bind(draft.source_type)
    .bind(draft.source_id)
    .bind(draft.currency.as_str())
    .execute(&mut **tx)
    .await?;

    for line in &draft.lines {
        let acct = account_id_by_code(tx, org_id, line.account_code).await?;
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
        .await?;
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
