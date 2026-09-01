# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–4.5 implemented; Phase 1.11 mobile/desktop shells.

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
| 1.11  | Flutter mobile + Tauri desktop shells (high-frequency set; push/biometric fakes in CI)                                          |
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
| 3.5   | Depth / polish: CRM orders/contracts/territories; finance tax/dunning/entities; ops timesheets/capacity; AI insights/meetings; list_invoices N+1 fix + RED meters |
| 4.1   | Multi-region foundations: region catalogue, org.region (ADR-015), cell routing, residency guards, failover drill (CI), control-plane region map |
| 4.2   | Enterprise multi-tenancy: IC + consolidation, hierarchy grants/delegation, CMEK (mock KMS), SCIM (2 mock IdPs), network allowlist/infra tier, SLA, eDiscovery |
| 4.3   | AI automation and agents: governed policy, receivables chase, kill switch, NL workflow drafts, prompt packs, review pack |
| 4.4   | Low-code builder: custom entities/fields, views/layouts, formulas, capped script sandbox, packaging + upgrade rehearsal |
| 4.5   | Industry packs (config + marketplace) + web offline/conflict parity matrix (Flutter/Tauri deferred to 1.11) |

## Phase 1.11 — Mobile and desktop shells (this PR)

- Flutter (`apps/mobile`): auth, org switch, dashboard, approvals, tasks, deal quick-updates, camera-first expense capture, push + biometric **interfaces with fakes**, offline read cache + queued mutations with stable `Idempotency-Key`
- Bottom tabs: Home · Work · Create · Inbox · More; pull-to-refresh; native-feel tab transitions
- Tauri (`apps/desktop`): wraps existing web app — system tray, native notifications API, global copilot hotkey Alt+Space, deep links `companyos://record/{id}`, offline shell with last cached dashboard
- Backend minimum: `POST/GET/DELETE /api/v1/notifications/devices` push token registration (no live FCM/APNs)
- CI: Flutter analyze + tests (+ optional unsigned Android APK); desktop `shell-core` unit tests. Store-signed iOS/macOS and crash reporting are follow-ups
- Parity matrix updated in `docs/clients/parity-matrix.md`

## Phase 4.5 — Industry packs / client parity (this PR)

- Four vertical packs as `companyos.custom.package` + seed + marketplace `industry.*` listings — **no** CRM/finance/HR/inventory forks
- Install / uninstall via `/api/v1/custom/industry-packs/...` (Member denied; uninstall retains tenant data)
- Grep lint: business services must not `match industry` / `if pack ==`
- Client parity matrix in `docs/clients/parity-matrix.md` (web implemented; native shells shipped in 1.11)
- Web offline-first: read cache, mutation queue with `Idempotency-Key`, reconnect replay, user-visible conflict UI (last-write-wins + `If-Match`)
- Custom record PATCH requires `If-Match` version

## Phase 4.4 — Low-code builder (done)

- New platform service `companyos-custom` (`/api/v1/custom/...`): entity definitions, records, views, layouts, scripts, packages
- Dynamic authz `custom.{slug}.read|write` registered on publish; Member denied `custom.builder.manage` by default
- Purpose-built JSON AST sandbox (CPU/memory/wall caps; no host eval; network/disk/env denied)
- Deterministic same-record formulas; money as integer minor units
- Versioned `companyos.custom.package` export/import; CI upgrade rehearsal via additive migrate `002`
- Events `companyos.{org}.custom.{entity}.{event}.v1` + search doc path `custom:{slug}`

## Phase 4.3 — AI automation and agents (done)

- First governed exception to ADR-012 propose-then-commit: unattended writes only inside a declared policy
- Effective perms = policy allow-list ∩ on_behalf_of ∩ org roles; sole PDP `crates/authz`; `ai_action` ledger
- Receivables chase agent (mock LLM); org kill switch ≤ 2s CI bound; monthly budget hard-stop
- NL → 3.1 workflow **draft** (human publishes); tenant prompt-pack (no real fine-tunes)
- Review report + fixture for error-rate threshold; Settings → AI → Agents monitor

## Phase 4.2 — Enterprise multi-tenancy (done)

- Consolidation on existing `finance_entity` (3.5): intercompany balanced pairs, elimination runs (same currency)
- Hierarchy inherited grants + membership delegation via sole PDP `crates/authz` + `policy_version`
- CMEK: customer wrap key for per-org DEK (`companyos-crypto` MockKms in CI) — rotate + revoke
- SCIM 2.0 Users/Groups with org-scoped bearer tokens (dedicated; not public API-key allowlist)
- Network allowlist + `infra_tier` fail-closed (gateway → internal network-gate); SLA targets/report
- Legal hold + durable `export_job` pack with audit hash-chain verification

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
- Gateway proxies `/api/v1/marketplace/*` and `/api/v1/integrations/*` to integration-service;
  OAuth token + authorize-permission endpoints are unauthenticated at the gateway (client
  credentials / opaque app tokens).
- Web UI: catalogue, listing consent, installs, publisher, reviewer, and Settings →
  Integrations (first-party connectors via the same install APIs).

Phase 4.5 adds industry pack listings (`industry.*`) as first-party catalogue entries.

Not yet done: a real connector runtime (the five seeded connectors are catalogue entries
only), cross-org publisher review staffing, and marketplace billing.

## Later phases

| Phase          | Notes                   |
| -------------- | ----------------------- |
| InvoiceDunning | Temporal activity wiring (catalogue configurable in 3.5) |
| PDF / email    | Nice-to-have            |
| Client parity  | Broader Flutter feature depth vs Phase 2–4 modules; store signing |

## Cut order if needed

Cut NL authoring, full BPMN import, cross-org marketplace templates, arbitrary HTTP webhook
actions (3.3), and visual debug time-travel before definition+versioning, event triggers,
permission-checked actions, Temporal execution, dry-run, monitor, and runaway bounds.
