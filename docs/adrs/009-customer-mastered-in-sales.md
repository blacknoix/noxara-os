# ADR 009: Customer mastered in Sales, projected into Finance

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Sales owns the customer aggregate. Finance receives projections via events — no cross-context table reads.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
