# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–1.7 merged; Phase 1.8 is this slice.

## Completed

| Phase | Scope |
|-------|--------|
| 0 | Monorepo, hello slice, RLS, outbox, authz PDP, gateway stub, web shell stub |
| 1.1 | Identity & authentication (JWT, refresh, MFA, sessions, switch-org) |
| 1.2 | Workspace (orgs, members, roles, permissions, teams, invitations) |
| 1.3 | Application shell, design system, dashboard BFF, members saved views, axe CI |
| 1.4 | CRM / Sales (customers, pipeline, deals, quotes, import) |
| 1.5 | Finance v1 (invoices, payments, journal, expenses, quote→invoice) |
| 1.6 | Projects & Tasks / Operations (`companyos-project`) |
| 1.7 | Approval engine (operations / Temporal) |

## Phase 1.8 — Platform events, notifications, search, analytics, files (this slice)

- Authz catalogue: `platform.notification.*`, `platform.search.*`, `platform.file.*`, `platform.analytics.*` (no member bypass)
- Gateway proxies `/api/v1/notifications|search|analytics|files` + SSE `/api/v1/notifications/stream` (Redis or feed poll)
- Notification service: ingest fan-out, prefs, feed, Redis PUBLISH, deferred digest
- Search indexer, analytics facts, file service (presign + local-upload / MinIO SigV4)
- Outbox relay binary + DLQ; services call `companyos_outbox::migrate`; optional `OUTBOX_EMBEDDED_RELAY=1`
- NATS bootstrap + DLQ replay scripts; outbox-lag runbook; event schema contract tests
- Web: TopBar notifications, CommandBar search, expense receipt upload
- OpenAPI merge includes platform services; `dev-up` starts platform binaries

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 1.9 | AI copilot in context panel |
| InvoiceDunning | Temporal dunning (separate from this approval engine) |
| PDF / email | Nice-to-have; local logs payment URL |
| Live provider | Stub webhook only |
| Mobile | Flutter / Tauri — inbox is responsive at existing breakpoints |

## Cut order if needed

Cut fancy search ranking, ClickHouse-backed analytics UI, and Temporal
NotificationDigest workflow before authz enforcement, gateway SSE, outbox
relay/DLQ, and feed/search/file wiring.
