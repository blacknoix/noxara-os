# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–2.6 merged; Phase 3.1 is this slice.

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
| 1.8 | Platform events, notifications, search, analytics, files, outbox relay |
| 1.9 | AI Assistant MVP (copilot, proposals, retrieval) |
| 2.1 | People / HR v1 (employees, onboarding/offboarding) |
| 2.2 | Attendance & Leave (People / hr-service) |
| 2.3 | Payroll basics (`companyos-hr`): draft → calculate → approve → paid, journals via Finance HTTP, Temporal `PayrollRun` (ADR 021) |
| 2.4 | Finance CoA, periods, bank rec, expense policy depth (ADR 022) |
| 2.5 | Inventory & Procurement (`companyos-inventory`) |
| 2.6 | Security & governance hardening (ABAC, field-level, access review, SSO, retention, API keys) |

## Phase 3.1 — Configurable workflow engine (this slice)

`companyos-workflow` (`/api/v1/workflows/...`) + Temporal catalogue `UserWorkflow`.

- Org-scoped definitions (`wfd_`) + immutable versions (`wfv_`) + instances (`wfi_`); RLS
- Triggers from existing domain events; actions call service APIs with `on_behalf_of` creator
- Permission check at save + every runtime action (deny by default; cannot exceed creator)
- Version pin: in-flight keeps started version; publish does not mutate running instances
- Runaway bounds: per-org concurrency + per-instance step cap (fail closed)
- Simulation/dry-run: zero DB/outbox/HTTP side effects
- Monitor: running / waiting / failed / SLA breaches
- AI must not auto-publish (human `operations.workflow.publish`)

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 3.2 | Analytics depth |
| 3.3 | Public API / webhooks |
| 3.4 | Marketplace |
| 3.5 | Depth / polish |
| InvoiceDunning | Temporal dunning polish |
| PDF / email | Nice-to-have |
| Mobile | Flutter / Tauri |
| 4.x | SCIM |

## Cut order if needed

Cut NL authoring, full BPMN import, cross-org marketplace templates, arbitrary HTTP webhook
actions (3.3), and visual debug time-travel before definition+versioning, event triggers,
permission-checked actions, Temporal execution, dry-run, monitor, and runaway bounds.
