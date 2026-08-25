# ADR 004: Temporal for long-running processes

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Approvals, onboarding, dunning, imports, and other long processes are Temporal workflows — not status columns plus cron.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
