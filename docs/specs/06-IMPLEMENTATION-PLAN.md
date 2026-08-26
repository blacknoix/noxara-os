# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–1.5 merged; Phase 1.6 is this slice.

## Completed

| Phase | Scope |
|-------|--------|
| 0 | Monorepo, hello slice, RLS, outbox, authz PDP, gateway stub, web shell stub |
| 1.1 | Identity & authentication (JWT, refresh, MFA, sessions, switch-org) |
| 1.2 | Workspace (orgs, members, roles, permissions, teams, invitations) |
| 1.3 | Application shell, design system, dashboard BFF, members saved views, axe CI |
| 1.4 | CRM / Sales (customers, pipeline, deals, quotes, import) |
| 1.5 | Finance v1 (invoices, payments, journal, expenses, quote→invoice) |

## Phase 1.6 — Projects & Tasks / Operations (this slice)

- Standalone `companyos-project` on port 8084 (`/api/v1/operations/...`)
- Gateway proxies `/api/v1/operations/*` (JWT; service enforces `operations.*`)
- Projects + tasks with soft delete, `If-Match` optimistic concurrency, five-column board
- Mentions → `operations_notification_intent` only for recipients with `operations.task.read`
- My Work (`GET /api/v1/operations/my-work`) via `operations_task_my_work_idx` (org_id, assignee_id, …)
- DealWon sales event projection → create project (idempotent by deal_id); no CRM table reads
- Outbox: `operations.task.created|assigned|completed.v1`, `operations.project.created.v1`
- Web: `/ops/projects`, `/ops/tasks` (board/list/calendar), `/my-work`; sidebar Ops + Work perms
- OpenAPI merge: core + CRM + Finance + Operations → `packages/sdk/openapi.json`
- Integration tests in `project-service/tests/operations_phase16.rs`

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 1.7 | Temporal approval engine / InvoiceDunning |
| 1.9 | AI copilot in context panel |
| PDF / email | Nice-to-have; local logs payment URL |
| Live provider | Stub webhook only |
| Mobile | Flutter / Tauri |

## Cut order if needed

Cut calendar polish, attachment uploads, and fancy board animations before RLS,
authz scopes, DealWon idempotency, outbox events, My Work index, and mention
filtering.
