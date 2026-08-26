# ADR 019: Finance double-entry ledger layout (Phase 1.5)

- Status: **Proposed** (implementation follows these posting rules; a finance reviewer has **not** signed off)
- Date: 2026-08-26

## Context

Finance v1 needs an immutable journal from day one (ADR 008 direction; CompanyOS invariants). Invoice issue, payment allocation, credit notes, and expenses must post balanced entries without inventing a full GL product.

## Decision

Seed a minimal chart of accounts per org:

| Code | Name | Type |
|------|------|------|
| 1000 | Cash | asset |
| 1100 | Accounts Receivable | asset |
| 2100 | Tax Payable | liability |
| 2200 | Customer Credits | liability |
| 4000 | Revenue | revenue |
| 5000 | Operating Expenses | expense |

Posting rules (document currency; FX captured on the invoice at issue for base reporting):

1. **Invoice issue** — Dr AR (total), Cr Revenue (net), Cr Tax Payable (tax).
2. **Payment** — Dr Cash (full receipt), Cr AR (allocated), Cr Customer Credits (unapplied / overpayment).
3. **Credit note** — Dr Revenue (net), Dr Tax Payable (tax), Cr AR (total).
4. **Expense posted** — Dr Expense, Cr Cash.

Journal entries and lines are append-only (DB triggers reject UPDATE/DELETE). Corrections are new documents / new entries.

## Consequences

- Debit = credit is enforced in application code before insert and covered by unit + proptest tests.
- Fancy multi-book / multi-entity ledgers are out of scope for 1.5.
- A named finance reviewer has not approved these mappings; treat them as engineering defaults documented here.
