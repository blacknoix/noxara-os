# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–3.2 implemented; Phase 3.3 is next.

## Completed

| Phase | Scope                                                                                                                           |
| ----- | ------------------------------------------------------------------------------------------------------------------------------- |
| 0     | Monorepo, hello slice, RLS, outbox, authz PDP, gateway stub, web shell stub                                                     |
| 1.1   | Identity & authentication (JWT, refresh, MFA, sessions, switch-org)                                                             |
| 1.2   | Workspace (orgs, members, roles, permissions, teams, invitations)                                                               |
| 1.3   | Application shell, design system, dashboard BFF, members saved views, axe CI                                                    |
| 1.4   | CRM / Sales (customers, pipeline, deals, quotes, import)                                                                        |
| 1.5   | Finance v1 (invoices, payments, journal, expenses, quote→invoice)                                                               |
| 1.6   | Projects & Tasks / Operations (`companyos-project`)                                                                             |
| 1.7   | Approval engine (operations / Temporal)                                                                                         |
| 1.8   | Platform events, notifications, search, analytics, files, outbox relay                                                          |
| 1.9   | AI Assistant MVP (copilot, proposals, retrieval)                                                                                |
| 2.1   | People / HR v1 (employees, onboarding/offboarding)                                                                              |
| 2.2   | Attendance & Leave (People / hr-service)                                                                                        |
| 2.3   | Payroll basics (`companyos-hr`): draft → calculate → approve → paid, journals via Finance HTTP, Temporal `PayrollRun` (ADR 021) |
| 2.4   | Finance CoA, periods, bank rec, expense policy depth (ADR 022)                                                                  |
| 2.5   | Inventory & Procurement (`companyos-inventory`)                                                                                 |
| 2.6   | Security & governance hardening (ABAC, field-level, access review, SSO, retention, API keys)                                    |
| 3.1   | Configurable workflow definitions, immutable versions, permission-checked actions, simulation, bounds, and monitoring           |
| 3.2   | Governed event-derived analytics, reports, dashboards, forecasts, exports, and scheduled delivery                               |

## Phase 3.2 — Analytics & reporting (done)

`companyos-analytics` (`/api/v1/analytics/...`) + Temporal catalogue
`ScheduledReportDelivery`.

- Governed metric catalogue shared by API, generated SDK, reports, dashboards, and UI
- Event-only typed facts for finance, sales, operations, and AI usage; no OLTP warehouse scans
- Mandatory `org_id` query guard, PostgreSQL RLS, and permission-filtered fact rows
- Saved reports with table/bar views, drill-through links, CSV export, and dry-run simulation
- Dashboards whose widgets bind only to governed metric names
- Explainable trailing-average and linear-trend forecasts exposing method and inputs
- Scheduled report generation/export/notification with an org-prefixed workflow ID
- Benchmarks, freshness/eventual-consistency labelling, and permission-aware Insights pages
- PostgreSQL mirror fixtures in CI; optional ClickHouse event sink and DDL in the full profile

## Later phases

| Phase          | Notes                   |
| -------------- | ----------------------- |
| 3.3            | Public API / webhooks   |
| 3.4            | Marketplace             |
| 3.5            | Depth / polish          |
| InvoiceDunning | Temporal dunning polish |
| PDF / email    | Nice-to-have            |
| Mobile         | Flutter / Tauri         |
| 4.x            | SCIM                    |

## Cut order if needed

Cut NL authoring, full BPMN import, cross-org marketplace templates, arbitrary HTTP webhook
actions (3.3), and visual debug time-travel before definition+versioning, event triggers,
permission-checked actions, Temporal execution, dry-run, monitor, and runaway bounds.
