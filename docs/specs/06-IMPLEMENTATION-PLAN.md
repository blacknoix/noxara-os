# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–2.2 merged; Phase 2.3 is this slice.

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

## Phase 2.3 — Payroll basics (`companyos-hr`) (this slice)

- Authz: `hr.payroll.read|write|approve|run`, `finance.journal.post`
- Own schema (`people_payroll_*`), API `/api/v1/people/payroll/...`, events under `Context::People`
- Lifecycle: draft → calculate → review → approve → paid; approved immutable; adjustments are new runs
- Attendance + unpaid leave feed calculate; every payslip line has `calculation_basis`
- Journals via Finance HTTP (`POST /api/v1/finance/journals`, source_type `payroll`) — no finance table writes from HR
- Approval: hybrid ApprovalProcess `payroll_run` + `hr.payroll.approve` (ADR 021)
- Temporal: `PayrollRun` — workflow id `{org}:PayrollRun:{run_id}`
- UI: payroll runs, payslip detail with basis, my payslips, CSV payment export
- Cut: full statutory filing, multi-country tax engines, live bank payouts, benefits marketplace, parallel-run vs historical file

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 2.4 | CoA / month-end / bank rec / expense policy — **this PR** |
| 2.5 | Inventory |
| InvoiceDunning | Temporal dunning polish |
| PDF / email | Nice-to-have |
| Mobile | Flutter / Tauri |

## Cut order if needed

Cut full statutory filing (TDS/EIN/HMRC), multi-country tax engines, live bank
payouts, and benefits marketplace before immutable runs, traceable payslips,
leave/attendance-aware calc, journal post, self-service, and gated+audited salary reads.
