# ADR 007: Money as integer minor units

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Finance amounts use `amount_minor: i64` plus ISO 4217 currency. No `f64` on the finance path. Half-up at document totals; largest-remainder allocation for splits.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
