# ADR 012: AI caller authority; propose-then-commit

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

AI calls the same APIs and `authz` policies as humans. v1 AI writes are previewed diffs — tagged, cited, reversible — and require human commit.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
