# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–1.8 merged; Phase 1.9 is this slice.

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

## Phase 1.9 — AI copilot (`companyos-ai`) (this slice)

- Authz catalogue: `ai.copilot.use`, `ai.proposal.create`, `ai.proposal.commit`, `ai.settings.*`, `ai.insights.read`, `ai.document.extract`
- Gateway proxy `/api/v1/ai/*` → `AI_SERVICE_URL` (`:8092`)
- AI service: chat/ask/stream, hybrid retrieval (org_id required), tool registry with authz trace, propose-then-commit writes, insights, document extract, org settings
- Mock LLM when `AI_API_KEY` unset; OpenAI-compatible when set
- Integration tests: `services/ai/ai-service/tests/phase19_ai.rs` (authz deny, retrieval tenant guard, injection, proposal pending until confirm)
- OpenAPI merge includes AI; `dev-up` starts `companyos-ai`; SDK types for AI DTOs

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| InvoiceDunning | Temporal dunning (separate from this approval engine) |
| PDF / email | Nice-to-have; local logs payment URL |
| Live provider | Stub webhook only |
| Mobile | Flutter / Tauri — inbox is responsive at existing breakpoints |

## Cut order if needed

Cut fancy search ranking, ClickHouse-backed analytics UI, and Temporal
NotificationDigest workflow before authz enforcement, gateway SSE, outbox
relay/DLQ, and feed/search/file wiring.
