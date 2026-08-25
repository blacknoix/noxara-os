# ADR 003: Transactional outbox + NATS JetStream

- Status: **Accepted**
- Date: 2026-03-25

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Domain writes and `outbox_event` rows commit in one transaction. A publisher delivers at-least-once to NATS JetStream. Consumers are idempotent via `idempotency_key`.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
