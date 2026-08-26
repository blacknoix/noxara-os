# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–1.4 merged; Phase 1.5 is this slice.

## Completed

| Phase | Scope |
|-------|--------|
| 0 | Monorepo, hello slice, RLS, outbox, authz PDP, gateway stub, web shell stub |
| 1.1 | Identity & authentication (JWT, refresh, MFA, sessions, switch-org) |
| 1.2 | Workspace (orgs, members, roles, permissions, teams, invitations) |
| 1.3 | Application shell, design system, dashboard BFF, members saved views, axe CI |
| 1.4 | CRM / Sales (customers, pipeline, deals, quotes, import) |

## Phase 1.5 — Finance v1 (this slice)

- Standalone `companyos-finance` on port 8083 (`/api/v1/finance/...`)
- Gateway proxies `/api/v1/finance/*` (JWT; finance enforces `finance.*`)
- Double-entry journal, immutable issued docs, gapless invoice numbers
- Customer projection from Sales events (no CRM table reads)
- Quote → invoice via snapshot API; CRM invoice-action CTA enabled when accepted
- Payments + Stripe-like webhook idempotency (fixtures; no live keys / card data)
- Expenses with workspace `approval_limit` (no Temporal)
- Dashboard finance widgets from real aggregates (`as_of`)
- OpenAPI merge: core + CRM + Finance → `packages/sdk/openapi.json`

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 1.7 | Temporal approval engine / InvoiceDunning |
| 1.9 | AI copilot in context panel |
| PDF / email | Nice-to-have; local logs payment URL |
| Live provider | Stub webhook only |
| Mobile | Flutter / Tauri |

## Cut order if needed

Cut PDF, email, live provider, fancy cash-flow charts, full recurring Temporal
before journal, immutability, gapless numbers, quote→invoice, webhook
idempotency, RLS, and authz.
