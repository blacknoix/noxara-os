# CompanyOS (noxara-os)

AI-native, multi-tenant **Business Operating System**.

This repository is the system source for product docs, Rust services, the Next.js web client, shared crates/packages, infrastructure skeletons, and the Rust → OpenAPI → TypeScript contract chain.

Product name in docs: **CompanyOS**. Crate/npm names may use `companyos-*` / `@companyos/*`; the GitHub repo is `noxara-os`.

## Phase 0 status

Phase 0 foundations only — **not** CRM, finance product features, or AI copilot.

You can:

1. Clone the repo and install toolchains (Rust, pnpm, Docker).
2. Run `scripts/dev-up` / `make dev-up` to start local dependencies + seed one org / two users.
3. Hit the **hello** vertical slice through the gateway BFF.
4. Open the web shell (`apps/web`) with loading / empty / error dashboard states.
5. Ship a PR that clears the nine-gate DoD checklist.

## Non-negotiable invariants

See [docs/00-INDEX.md](docs/00-INDEX.md). Summary:

1. One tenant key: `org_id` + PostgreSQL RLS everywhere.
2. One PDP: `crates/authz` (deny by default; explicit deny wins).
3. Bounded contexts own their data.
4. Write + outbox atomically; NATS JetStream at-least-once; idempotent consumers.
5. Long processes → Temporal workflows.
6. Money = `amount_minor: i64` + ISO 4217 (never `f64`).
7. Everything attributable (AI records the human it acts for).
8. AI proposes, humans commit (v1).

## Monorepo layout

```text
apps/web/                 Next.js App Router shell
services/gateway/         Axum API gateway / BFF
services/core/            Auth/org/user/audit home — Phase 0 hello service
services/business|platform|ai/   Placeholders (split by config later)
crates/                   ids, money, errors, telemetry, tenancy, events, outbox, authz, testkit
packages/design-system/   Tokens + primitives
packages/sdk/             OpenAPI + TypeScript SDK stub
infrastructure/docker/    Compose for local deps
infrastructure/terraform/ Skeletons only (no live cloud)
docs/                     Specs, ADRs, runbooks
scripts/                  One-command bootstrap
```

## Quick start

```bash
cp .env.example .env
make dev-up
pnpm install
pnpm --filter @companyos/web dev
```

**LOCAL-ONLY auth** (never production):

```bash
source .tmp/seed.env
curl -s http://127.0.0.1:8080/api/v1/hello \
  -H "X-CompanyOS-Dev-Org-Id: $DEV_ORG_PUBLIC_ID" \
  -H "X-CompanyOS-Dev-User-Id: $DEV_USER_OWNER_PUBLIC_ID"
```

OpenSearch and ClickHouse are optional (`docker compose --profile full`). The hello path must work if they are down.

## Toolchain

- `cargo fmt` / `cargo clippy -D warnings` / `cargo test --workspace`
- `pnpm typecheck` / `pnpm lint` / Prettier
- TypeScript **strict**
- EditorConfig at repo root

## Contract chain

Rust hello types → OpenAPI 3.1 (`/api/v1/openapi.json`) → `packages/sdk` TypeScript.  
CI fails on schema drift (`pnpm check:openapi-drift`).

## Docs

- [Documentation index](docs/00-INDEX.md)
- [CONTRIBUTING](CONTRIBUTING.md)
- ADRs 001–015 under `docs/adrs/`

## Out of scope (Phase 0)

OAuth/MFA/SSO product, CRM, invoices, payments, projects, AI copilot, Flutter, Tauri, live AWS/K8s, real Temporal workflows beyond compose being up.
