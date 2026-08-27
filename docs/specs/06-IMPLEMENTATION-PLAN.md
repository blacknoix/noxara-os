# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–2.1 merged; Phase 2.2 is this slice.

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

## Phase 2.1 — People / HR v1 (`companyos-hr`)

- Authz: `hr.employee.read|read_sensitive|write|onboard|offboard`, `hr.document.read|write`
- Own schema (`people_*`), API `/api/v1/people/...`, events under `Context::People`
- Restricted-field AES-GCM encryption; money as `amount_minor` + ISO currency
- Temporal catalogue: `EmployeeOnboarding` / `EmployeeOffboarding` (workflow ids `{org}:…:{emp_}`)
- Offboarding access checklist (membership + sessions; API keys / integration tokens N/A)
- Departments: Workspace SoT (ADR 020); reporting line mastered in People
- UI: People directory, employee record tabs, onboard, self-service profile
- Search indexer: `employee` → `hr.employee.read`
- Cut: ATS, performance reviews, asset depreciation, fancy org-chart — before employee master, encryption+authz, onboarding/offboarding, directory, access checklist

## Phase 2.2 — Attendance & Leave (`companyos-hr`) (this slice)

- Authz: `hr.attendance.read|write`, `hr.leave.read|write|approve`
- Append-only attendance (`att_`); schedules (`sch_`) / holidays (`hol_`)
- Leave types (`lvt_`), ledger-true balances (`lv_`), requests (`lvr_`) via ApprovalProcess `leave_request`
- Temporal catalogue: `LeaveCarryForward` — workflow id `{org}:LeaveCarryForward:{year}`
- UI: attendance clock/list, my leave, team calendar, balances, leave types, schedules
- Cut: geo fencing / live tracking, biometric clocks, statutory packs, payroll posting from leave

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 2.3 | Payroll calculation |
| 2.4 | CoA / month-end |
| 2.5 | Inventory |
| InvoiceDunning | Temporal dunning polish |
| PDF / email | Nice-to-have |
| Mobile | Flutter / Tauri |

## Cut order if needed

Cut ATS, performance reviews, asset depreciation, and fancy org-chart viz before
employee master data, sensitive-field encryption + authz, onboarding/offboarding
workflows, directory, and the offboarding access checklist.
