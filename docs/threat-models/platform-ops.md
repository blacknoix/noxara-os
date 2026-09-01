# Threat model — Platform / ops surface (Phases 1–4 + 1.11 refresh)

Companion to [`auth.md`](./auth.md). Covers attack surface after enterprise
multi-tenancy, AI agents, industry packs, and mobile/desktop shells.

## Assets

- Tenant OLTP (Postgres under RLS) and backups
- Outbox / NATS event stream and DLQ
- Search index (OpenSearch) + Postgres `search_doc_mirror`
- Analytics facts (Postgres mirror; optional ClickHouse ingest)
- JWKS signing material and CMEK-wrapped DEKs
- SCIM bearer tokens; API keys; refresh token families
- AI prompts, proposals, agent action ledger
- Mobile/desktop offline caches and mutation queues

## Trust boundaries

| Boundary | Notes |
|----------|-------|
| Clients ↔ Gateway | TLS in prod; JWT or API key; cell/region gate; network allowlist |
| Gateway ↔ services | Private network; coarse authz + per-service PDP |
| Services ↔ Postgres | Non-superuser; `app.org_id`; FORCE RLS |
| Relay ↔ NATS | At-least-once; consumers idempotent |
| AI ↔ LLM provider | Optional; mock in CI; kill switch ≤2s |
| Enterprise IdP / SCIM | Customer-controlled; mocked in CI |
| KMS | MockKms in CI; real cloud KMS out of scope here |

## Key threats & controls

1. **Cross-tenant read/write** — RLS + org-scoped JWT + planted isolation tests; restore drill re-asserts RLS.
2. **Audit log tampering** — hash chain + fail-closed verify.
3. **Event loss / poison** — transactional outbox; DLQ + replay runbooks; NATS-down game day (writes continue).
4. **Search/analytics dependency outage** — degradation ladder (Postgres fallback / mirror + banners).
5. **AI runaway spend / writes** — budget hard-stop, kill switch, propose-then-commit (agents governed).
6. **Secret leakage** — rotation runbooks; CI rotates MockKms only; no live AI keys in CI.
7. **Region residency bypass** — ADR-015 region attribute; gateway cell gate; EU fail-closed drill.
8. **Offline client replay / conflict** — Idempotency-Key queues; conflict UI; no second TaskDto.
9. **Privilege escalation via SCIM/SSO** — enterprise feature gates; dual mock IdP tests.
10. **Backup restore into wrong tenancy context** — isolated restore DB + non-superuser assertions.

## Pen-test appendix (for later engagement — not executed here)

### Attack surface summary

- Public HTTP: gateway auth, public API allowlist, webhooks, SCIM endpoints
- Auth: password, MFA, SSO OIDC, refresh families, API keys, JWKS
- Data plane: CRM/finance/HR/inventory/ops, files, search, analytics, AI
- Ops plane: outbox relay, Temporal workers, control-plane region map
- Clients: web, Flutter mobile, Tauri desktop (unsigned in CI)

### Suggested test plan (external)

1. Tenant isolation fuzz (JWT org swap, missing `org_id`, SQL injection on filters)
2. Authz deny-matrix vs intentional grants (no catalogue drift)
3. Webhook / SCIM auth bypass and replay
4. Rate-limit and lockout bypass
5. AI prompt injection → unauthorized tool execution
6. Backup/artifact access controls in staging
7. Mobile deep-link / offline queue tampering

### Out of scope for this pack

Hiring/running the external pen test; live AWS multi-region; real KMS; App Store builds.
