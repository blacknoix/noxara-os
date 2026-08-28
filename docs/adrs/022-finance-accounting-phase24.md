# ADR 022: Finance CoA, periods, and bank reconciliation (Phase 2.4)

- Status: **Accepted**
- Date: 2026-08-27

## Context

Phase 1.5 shipped an append-only journal with a minimal seeded CoA (ADR 019).
Phase 2.3 added payroll posting via Finance HTTP (`5100` / `2300` / `2400`) with a
payroll-only unique index on `(org_id, source_type, source_id)`. Product needs
user-facing chart of accounts, fiscal period close, trial balance / P&L / BS,
bank statement import + reconciliation, and deeper expense policy without
rebuilding invoices/payments/expenses v1.

## Decision

1. **Finance owns** CoA, journals, periods, bank rec, and expense policy — own
   tables (`finance_*`), own API (`/api/v1/finance/...`), own events. No
   cross-context table reads. HR continues to post journals through Finance HTTP.
2. **Periods** are open / closed / locked. Closed and locked periods reject
   postings with RFC 9457 `conflict`. Reopen is explicit (`finance.period.reopen`),
   requires a reason, and is audited.
3. **Manual journals** must balance on write; posted lines remain immutable
   (reversing entries only).
4. **Payroll unique index stays payroll-only** — do not restore a broad unique on
   `(org_id, source_type, source_id)` (broke payment allocation in 2.3).
5. **Money** remains `amount_minor: i64` + ISO currency. Trial balance is a
   continuous assertion: sum(debits) == sum(credits) per org/period.
6. **Bank rec** imports CSV statements and auto-matches on amount + date
   (±3 days) + optional reference (target ≥90% on fixture).
7. **Expense policy** adds per-category limits with `require_approval` or
   `reject`; mileage / per-diem / card import / reimbursement batches deepen
   expenses without replacing the 1.5/1.7 approval path.

## Consequences

- Authz catalogue gains CoA / period / bank / policy / reimbursement permissions;
  `crates/authz` remains the sole PDP.
- Event subjects follow
  `companyos.{org_id}.finance.{aggregate}.{event}.v1`.
- Multi-entity consolidation, live bank feeds, ML matching, and statutory
  close packs remain out of scope.
