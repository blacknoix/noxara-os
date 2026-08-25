# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–1.3 merged; Phase 1.4 is this slice.

## Completed

| Phase | Scope |
|-------|--------|
| 0 | Monorepo, hello slice, RLS, outbox, authz PDP, gateway stub, web shell stub |
| 1.1 | Identity & authentication (JWT, refresh, MFA, sessions, switch-org) |
| 1.2 | Workspace (orgs, members, roles, permissions, teams, invitations) |
| 1.3 | Application shell, design system, dashboard BFF, members saved views, axe CI |

## Phase 1.4 — CRM / Sales integration (this slice)

- Standalone `companyos-crm` service on port 8082 (`/api/v1/sales/...`)
- Gateway proxies `/api/v1/sales/*` to CRM with JWT auth (coarse; CRM enforces `sales.*`)
- Dashboard BFF pipeline widget fetches `GET /api/v1/sales/reports/summary` when CRM is up
- OpenAPI merge: core + CRM → `packages/sdk/openapi.json`
- `scripts/dev-up` starts core, CRM, and gateway with `CRM_SERVICE_URL`

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| Finance | Invoices, payments (no invented numbers until then) |
| 1.9 | AI copilot in context panel; command-bar ask-mode becomes real |
| Mobile | Flutter / Tauri — not Phase 1.4 |

## Cut order if needed

Cut charts / kanban polish / Storybook before Table, FilterBar, saved views,
shell, states, and axe CI.
