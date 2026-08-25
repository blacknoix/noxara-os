# ADR 014: Soft delete + retention; hard delete via workflow

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Default delete is soft with retention. Hard delete is a Temporal workflow with audit.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
