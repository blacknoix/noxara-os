# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–1.6 merged; Phase 1.7 is this slice.

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

## Phase 1.7 — Approval engine (this slice)

- Generic `operations_approval` + `operations_approval_step` + versioned `operations_approval_policy`
- Authz: `operations.approval.read|decide|manage` (catalogue = sole PDP)
- Routing by amount, category, department, requester role; modes sequential / any / all
- Delegation (window or per-request); SLA timer + escalation in `ApprovalProcess` Temporal workflow
- Worker binary `companyos-project-worker` on task queue `companyos-approvals` (compose already has Temporal)
- Workflow ID `{org_id}:ApprovalProcess:{approval_id}` (idempotent); workflows never touch DB
- Events: `operations.approval.requested.v1`, `operations.approval.decided.v1` (write+outbox one TX)
- Policy version permanently recorded on each approval; policy publish never rewrites in-flight
- Duplicate decide / signal is a no-op
- Unified Approvals inbox (`/approvals`) + dashboard Approvals widget count
- Finance expenses above `approval_limit` call Operations approval API; CRM quote discounts ≥ threshold hold as `pending_approval`
- OpenAPI merge includes approval routes; gateway already proxies `/api/v1/operations/*`
- Tests: `approvals_phase17.rs` + `workflow_logic` unit tests (timer survives restart)

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 1.8 | Search, ClickHouse, notification fan-out product, full event platform |
| 1.9 | AI copilot in context panel |
| InvoiceDunning | Temporal dunning (separate from this approval engine) |
| PDF / email | Nice-to-have; local logs payment URL |
| Live provider | Stub webhook only |
| Mobile | Flutter / Tauri — inbox is responsive at existing breakpoints |

## Cut order if needed

Cut delegation UI polish, bulk approve, fancy rationale viz before Temporal workflow,
policy versioning, no double-decide, expense+discount hooks, inbox, RLS.
