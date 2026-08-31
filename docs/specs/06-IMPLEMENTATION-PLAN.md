# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–2.5 merged; Phase 2.6 is this slice.

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

## Phase 2.6 — Security & governance hardening (this slice)

Platform/governance — extend `crates/authz`, `companyos-core`, shared `audit_entry`. No new
bounded context; thin governance module under core. Own tables keep `org_id` + RLS.

- **ABAC** condition library (time, location, delegation, record state) wired into PDP
  `decide_with_context`; fail-closed; tests for time window + record state
- **Field-level permissions**: `hr.field.compensation_read|government_id_read|bank_read`,
  `finance.field.bank_account_read|salary_journal_read` — never bypass
  `hr.employee.read_sensitive`; UI hides fields the principal cannot read
- **Access review**: who-could / who-did from `permission_entitlement_history` + audited
  sensitive reads; kickoff + CSV/JSON export; DoD Q&A under two minutes
- **Audit**: append-only hash-chained partitions (DB trigger) + verify job fails closed
- **SSO**: OIDC login path (enterprise / feature-flag gated); dual mocked IdPs in tests;
  SCIM deferred to Phase 4
- **Retention**: per-org config + dry-run cutoff selection (no live prod-like purge)
- **Secrets**: hashed org API keys with create/rotate/revoke
- **SOC 2 Type I readiness**: `docs/compliance/` control mapping, DPIA + sub-processor templates
- API: `/api/v1/governance/...` via gateway; Idempotency-Key on mutating endpoints

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| 3.x | Configurable workflows, public API, marketplace — **do not start** |
| InvoiceDunning | Temporal dunning polish |
| PDF / email | Nice-to-have |
| Mobile | Flutter / Tauri |
| 4.x | SCIM |

## Cut order if needed

Cut live Okta+Azure AD in CI, CMEK, private connectivity, SCIM, and paid SOC 2 Type I
attestation before access-review Q&A, field-level HR/Finance, hash-chain verify, OIDC SSO
path + second mocked IdP, retention config, and in-repo control/evidence pack.
