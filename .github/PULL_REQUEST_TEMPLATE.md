## Summary

<!-- What does this PR change and why? -->

## Phase / scope

- [ ] Phase 0 foundations only (no Phase 1 product features)

## Definition of done (9 gates)

- [ ] **Functionality** — behavior matches the intended slice; happy path works locally
- [ ] **API** — OpenAPI/types updated; no contract drift (`pnpm check:openapi-drift`)
- [ ] **Tenancy** — `org_id` on rows/tokens/events; RLS session bound; cross-tenant tests pass
- [ ] **Authorization** — decisions go through `crates/authz` only; deny-by-default covered
- [ ] **Audit & events** — mutations audited; domain write + outbox in one transaction
- [ ] **Tests** — unit/integration as appropriate; tenant-isolation planted query fails loudly
- [ ] **UI slice** — loading / empty / error states (no fake CRM data in Phase 0)
- [ ] **Observability** — structured logs/traces; `/livez` `/readyz` `/healthz`; no secrets logged
- [ ] **Documentation** — README/ADR/runbook/spec touch-ups as needed

## LOCAL-ONLY auth

- [ ] Any auth used is clearly marked local-only and is not production-ready

## Test plan

<!-- Commands you ran -->
