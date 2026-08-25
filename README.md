# CompanyOS (noxara-os)

AI-native, multi-tenant **Business Operating System**.

This repository is the system source for product docs, Rust services, the Next.js web client, shared crates/packages, infrastructure skeletons, and the Rust → OpenAPI → TypeScript contract chain.

Product name in docs: **CompanyOS**. Crate/npm names may use `companyos-*` / `@companyos/*`; the GitHub repo is `noxara-os`.

## Phase status

- **Phase 0** foundations (merged): hello slice, RLS, outbox, authz PDP, gateway stub.
- **Phase 1.1** (merged): Identity & Authentication — org-scoped JWTs, refresh cookies, MFA, OAuth, sessions, switch-org.
- **Phase 1.2** (merged): Workspace — organizations, memberships, roles, permissions, teams, invitations, OrgProvisioning.
- **Phase 1.3** (merged): Application shell, design system, dashboard BFF, command bar, members saved views, axe CI.
- **Phase 1.4** (this line of work): CRM / Sales service (`companyos-crm`), gateway proxy for `/api/v1/sales/*`, dashboard pipeline widget, merged OpenAPI.

Not in scope yet: invoices / finance metrics, real AI copilot (1.9), full SSO IdP, Flutter/Tauri, live AWS.

## Non-negotiable invariants

See [docs/00-INDEX.md](docs/00-INDEX.md).

## Monorepo layout

```text
apps/web/                 Next.js App Router (auth pages + shell)
services/gateway/         Axum BFF — JWT authN, tenant headers, coarse authz, core + CRM proxy
services/core/            Auth + org/user/audit home + hello slice + dashboard BFF
services/business/crm-service/  CRM / Sales API (`/api/v1/sales/...`)
crates/                   ids, money, errors, telemetry, tenancy, events, outbox, authz, auth-token, testkit
packages/design-system/   Tokens + Table/FilterBar/shell primitives (gallery: /dev/components)
packages/sdk/             OpenAPI + TypeScript SDK
docs/                     Specs, ADRs, threat models, runbooks
```

## Quick start

```bash
cp .env.example .env
make dev-up          # postgres + deps; seeds Acme Demo
pnpm install
# terminal A
AUTH_JWT_SECRET=local-dev-only-change-me AUTH_COOKIE_SECURE=0 \
  cargo run -p companyos-core
# terminal B
AUTH_JWT_SECRET=local-dev-only-change-me COMPANYOS_LOCAL_AUTH=0 \
  cargo run -p companyos-gateway
# terminal C
pnpm --filter @companyos/web dev
```

### Auth locally

- Web: `/login`, `/signup`, `/verify-email`, `/magic-link`, `/mfa`, `/reset-password`
- Magic links / verification emails are **logged** and written under `.tmp/mail/` (or `AUTH_MAIL_DIR`). No real SMTP required locally.
- Seeded member: `member@acme.demo` / `correct-horse-battery` (no MFA).
- Seeded owner: same password but **MFA required** on login (enroll via `/mfa`).
- Refresh token: httpOnly cookie `companyos_refresh`. Access token: in-memory only (not localStorage).
- Breach checks: fixture list locally; set `HIBP_ENABLED=1` for Have I Been Pwned k-anonymity in prod.
- OAuth: set `GOOGLE_OAUTH_*` / `MICROSOFT_OAUTH_*`. Tests use `OAUTH_MOCK_BASE`.
- SSO admin API exists but returns `403 feature_disabled` unless `COMPANYOS_SSO_ENABLED=1` **and** org flag `sso`.

### LOCAL-ONLY bypass (default off)

Set `COMPANYOS_LOCAL_AUTH=1` only for scripts/tests:

```bash
source .tmp/seed.env
curl -s http://127.0.0.1:8080/api/v1/hello \
  -H "X-CompanyOS-Dev-Org-Id: $DEV_ORG_PUBLIC_ID" \
  -H "X-CompanyOS-Dev-User-Id: $DEV_USER_MEMBER_PUBLIC_ID"
```

## Toolchain

- `cargo fmt` / `cargo clippy -D warnings` / `cargo test --workspace`
- `pnpm typecheck` / `pnpm lint`
- OpenAPI drift: `pnpm check:openapi-drift`
- A11y / unit: `pnpm test:a11y` / `pnpm test:unit`

## Docs

- [Documentation index](docs/00-INDEX.md)
- [Auth threat model](docs/threat-models/auth.md)
- Runbooks: key rotation, mass session revocation, locked-out owner
- ADR 016: org-scoped JWT + opaque refresh cookies
