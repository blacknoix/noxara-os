# ADR 001: Rust + Axum for services

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Services are implemented in Rust with Axum. Shared logic lives in workspace crates. Chosen for performance, memory safety, and strong typing across tenancy/money/authz.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
