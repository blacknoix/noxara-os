# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–3.3 implemented; Phase 3.4 backend skeleton landed; Phase 3.5 not started.

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
| 3.3   | Public API, generated SDKs (TS + Python), outbound webhooks, developer docs + sandbox                                           |
| 3.4   | Marketplace skeleton: listings, review gate, scoped-consent installs, app tokens, mock OAuth, integrations alias                |

## Phase 3.3 — Public API, SDKs & webhooks (done)

- Documented public subset of `/api/v1/...` with API-key auth (scopes ∩ owner role)
- OpenAPI public contract + TS (`packages/sdk`) and Python (`packages/sdk-python`) SDKs; CI drift
- Outbound org webhooks (`whk_` / `whd_`) with HMAC signing, SSRF fail-closed, retries, replay
- Per-key rate limits + usage analytics events; 180-day deprecation dual-publish exercise
- Developer docs under `docs/developers/` + `/developers` portal route; sandbox seed script

## Phase 3.4 — Marketplace (backend skeleton done)

Implemented in `companyos-integration` (`src/marketplace/`, `migrations/001_marketplace.sql`):

- Listings (`app_`), OAuth clients (`oac_`), reviews (`mrv_`), installs (`ins_`) and app
  tokens (`atk_`), all org-scoped under FORCE RLS. Published listings are readable across
  orgs; installs and tokens stay strictly tenant-isolated.
- Consent is the only authority: issued token scopes equal the install's consented scopes,
  which must be a subset of the listing's requested scopes intersected with the installing
  principal's own permissions. Widening consent revokes and re-issues.
- No first-party special case. `listing_kind` / `connector_key` are data; the
  `/api/v1/integrations/{connector_key}/{connect,disconnect}` routes are an alias over the
  same `create_install` / `issue_tokens` / `revoke_install` path used by third-party apps.
- Publication is blocked until every required checklist item is complete and the derived
  `security_review_completed` flag is true (three `security_*` items).
- Uninstall revokes the install, every access and refresh token, and both traffic
  directions; subsequent token authorization returns 401.
- Mock PKCE OAuth: authorize → single-use code → token exchange → refresh rotation, plus
  `POST /api/v1/marketplace/oauth/authorize-permission` for resource-server checks.
- Session JWT / local auth only — marketplace routes are not on the public API-key allowlist.

Not yet done: a real connector runtime (the five seeded connectors are catalogue entries
only), cross-org publisher review staffing, and marketplace billing.

## Later phases

| Phase          | Notes                   |
| -------------- | ----------------------- |
| 3.5            | Depth / polish          |
| InvoiceDunning | Temporal dunning polish |
| PDF / email    | Nice-to-have            |
| Mobile         | Flutter / Tauri         |
| 4.x            | SCIM                    |

## Cut order if needed

Cut NL authoring, full BPMN import, cross-org marketplace templates, arbitrary HTTP webhook
actions (3.3), and visual debug time-travel before definition+versioning, event triggers,
permission-checked actions, Temporal execution, dry-run, monitor, and runaway bounds.
