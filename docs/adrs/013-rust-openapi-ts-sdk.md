# ADR 013: Rust → OpenAPI → TypeScript SDK

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

API types originate in Rust, are published as OpenAPI 3.1, and generate/feed the TypeScript SDK. CI fails on drift.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
