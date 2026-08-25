# ADR 008: Double-entry from v1; user-facing accounting Phase 2

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

The journal is append-only and balances from v1. Full user-facing accounting UX is Phase 2.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
