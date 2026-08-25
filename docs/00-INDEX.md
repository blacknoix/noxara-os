# CompanyOS documentation index

CompanyOS (repo `noxara-os`) is an AI-native, multi-tenant Business Operating System.

## Non-negotiable invariants

1. **One tenant key:** `org_id` on every tenant-owned row, enforced by PostgreSQL RLS, present in every token, cache key, event subject, workflow ID, and analytics predicate.
2. **One policy decision point:** `crates/authz` decides every permission question for humans, workflows, and AI. Deny by default; explicit deny wins. Permission IDs are `{context}.{resource}.{action}`.
3. **Bounded contexts own their data.** FKs within a context; identifiers plus events across contexts. No cross-context table reads.
4. **Write and publish atomically.** Domain change + outbox event in one transaction; at-least-once to NATS JetStream; idempotent consumers.
5. **Long processes are Temporal workflows,** not status columns plus cron.
6. **Money is integer minor units** (`amount_minor: i64`) plus ISO 4217 currency. No floats on the finance path. Financial documents are immutable; corrections are new documents; journal is append-only and balances.
7. **Everything is attributable.** Every mutation writes an audit entry naming the actor, including AI which always records the human it acts on behalf of.
8. **AI proposes, humans commit (v1).** Every AI write is a previewed diff, tagged, cited, and reversible.

## Spec placeholders

| Doc | Status |
|-----|--------|
| [01-PRD](specs/01-PRD.md) | Placeholder — points at invariants |
| [02-TRD](specs/02-TRD.md) | Placeholder |
| [03-UIUX](specs/03-UIUX.md) | Placeholder |
| [04-APP-FLOW](specs/04-APP-FLOW.md) | Placeholder |
| [05-SCHEMA](specs/05-SCHEMA.md) | Placeholder |
| [06-IMPLEMENTATION-PLAN](specs/06-IMPLEMENTATION-PLAN.md) | Placeholder |

## ADRs

Accepted ADRs [001–015](adrs/) document foundational decisions.

## Other

- [CONTRIBUTING](../CONTRIBUTING.md)
- [API stubs](api/)
- [Events stubs](events/)
- [Runbooks](runbooks/)
- [Auth threat model](threat-models/auth.md)
- ADR [016](adrs/016-org-scoped-jwt-opaque-refresh.md) — org-scoped JWT + opaque refresh cookies
