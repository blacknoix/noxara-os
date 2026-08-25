# ADR 010: Service groups co-deployed in Phase 1

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Logical groups: gateway, core, business, platform, ai. Phase 1 may co-deploy; transport/network boundaries remain. Split is configuration-only.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
