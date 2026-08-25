# ADR 005: Single authz crate as sole PDP

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

`crates/authz` is the only policy decision point for humans, workflows, and AI. Deny by default; explicit deny wins. Permission IDs use `{context}.{resource}.{action}`.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
