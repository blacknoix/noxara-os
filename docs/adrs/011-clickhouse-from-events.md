# ADR 011: ClickHouse from events only

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Analytics derives from the event stream into ClickHouse. No direct reads of OLTP tables for warehouse loads.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
