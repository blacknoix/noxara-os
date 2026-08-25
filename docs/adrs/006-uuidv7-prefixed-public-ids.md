# ADR 006: UUIDv7 internals + prefixed public IDs

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Internal PKs are UUIDv7. API-facing IDs are prefixed (`org_`, `usr_`, `inv_`, `dl_`, `cus_`, …) and round-trip to the same UUID.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
