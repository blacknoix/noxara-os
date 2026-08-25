# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–1.2 merged; Phase 1.3 is this slice.

## Completed

| Phase | Scope |
|-------|--------|
| 0 | Monorepo, hello slice, RLS, outbox, authz PDP, gateway stub, web shell stub |
| 1.1 | Identity & authentication (JWT, refresh, MFA, sessions, switch-org) |
| 1.2 | Workspace (orgs, members, roles, permissions, teams, invitations) |

## Phase 1.3 — Application shell, design system, dashboard

- Upgrade `@companyos/design-system` tokens + primitives (Table, FilterBar, states, CommandBar, …)
- App shell: TopBar, grouped permission-aware Sidebar, Context panel (1.9 stub), ⌘K
- Dashboard BFF `GET /api/v1/dashboard` + honest empty widgets
- Gateway proxies `/api/v1/workspace/*` and `/api/v1/dashboard`
- Members collection: Table + FilterBar + URL saved views
- axe CI on shell / dashboard / members / login structures
- ADR 018 — table virtualisation library

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 1.4+ | CRM / pipeline boards |
| Finance | Invoices, payments (no invented numbers until then) |
| 1.9 | AI copilot in context panel; command-bar ask-mode becomes real |
| Mobile | Flutter / Tauri — not Phase 1.3 |

## Cut order if needed

Cut charts / kanban polish / Storybook before Table, FilterBar, saved views,
shell, states, and axe CI.
