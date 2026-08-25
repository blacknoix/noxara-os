# ADR 002: Shared schema + PostgreSQL RLS

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Tenant isolation is enforced in PostgreSQL with Row-Level Security on tenant-owned tables. Session variable `app.org_id` is set per request. Application filters are defense in depth, not the sole control.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
