# Contributing to CompanyOS (noxara-os)

## Prerequisites

- Rust stable (1.85+)
- pnpm 9+
- Docker (for `scripts/dev-up`) or local Postgres 16 + Redis + NATS
- PostgreSQL client (`psql`) for seed scripts

## Quick start (< 30 minutes)

```bash
git clone https://github.com/blacknoix/noxara-os.git && cd noxara-os
cp .env.example .env
make dev-up
# brings up Postgres/Redis/NATS/Temporal/MinIO, migrates, seeds Acme Demo,
# starts core + crm + finance + project + platform + ai + gateway

pnpm install
pnpm --filter @companyos/web dev   # http://127.0.0.1:3000 → API via gateway :8080
```

OpenSearch and ClickHouse are **optional** (`docker compose … --profile full`). Auth/CRM/finance work without them.

### Exact service commands (if not using the auto-started binaries)

Share the same `AUTH_JWT_SECRET` and keep `COMPANYOS_LOCAL_AUTH=0` (default):

```bash
export $(grep -v '^#' .env | xargs)
cargo run -p companyos-core              # :8081
cargo run -p companyos-crm               # :8082
cargo run -p companyos-finance           # :8083
cargo run -p companyos-project           # :8084
cargo run -p companyos-hr                # :8088 People / HR
cargo run -p companyos-notification      # :8085
cargo run -p companyos-search            # :8086
cargo run -p companyos-analytics         # :8087
cargo run -p companyos-file              # :8089
cargo run -p companyos-outbox-relay      # :8090
cargo run -p companyos-workflow-host     # :8091
cargo run -p companyos-ai                # :8092 (mock unless AI_API_KEY)
cargo run -p companyos-project-worker
cargo run -p companyos-gateway           # :8080
```

Seed: `bash scripts/seed-dev.sh` (org + two users + **OrgProvisioning**).  
Member: `member@acme.demo` / `correct-horse-battery`. Owner needs MFA.

Deal-to-cash walkthrough: [docs/runbooks/deal-to-cash.md](docs/runbooks/deal-to-cash.md).  
Automated: `cargo test -p companyos-finance --test deal_to_cash`.

## Authentication (Phase 1.1)

Primary path: signed org-scoped access JWTs + opaque refresh cookie.

- Access token: `Authorization: Bearer …` (web keeps it in memory)
- Refresh: httpOnly cookie `companyos_refresh` on `/api/v1/auth`
- Org switch: `POST /api/v1/auth/switch-org` → **new** access token
- MFA mandatory for Owner/Admin (policy in `crates/authz`)
- `COMPANYOS_LOCAL_AUTH=1` enables Phase 0 header/unsigned bypass (**default off**)

Local mail: links printed to logs and `.tmp/mail/`.

## Workspace (Phase 1.2)

- Organizations, memberships, invitations, teams/departments, roles + permission matrix
- `crates/authz` catalogue must match `permission_definition` (CI test)
- Last-Owner invariant enforced on revoke / suspend / demote
- OrgProvisioning durable command (ADR 017); Temporal worker is a follow-up
- Web: `/settings`, `/onboarding`, `/invite/accept`

DoD tests: `services/core/tests/workspace_phase12.rs` and `auth_phase11.rs`.

## Design system & shell (Phase 1.3)

Package: `packages/design-system` (`@companyos/design-system`).

- **Upgrade in place** — do not fork a second UI kit or parallel token set.
- Tokens: light / dark / high-contrast (`data-theme`), plus `tokens.css` and shared `styles.css` (focus rings, skip-link, reduced motion).
- One `Table` + `FilterBar` grammar everywhere; saved views serialize to URL (`q`, `f`, `view`). See ADR [018](docs/adrs/018-table-virtualisation.md).
- Component gallery (Storybook-equivalent): `/dev/components` in the web app.
- App shell: TopBar 56px, grouped permission-aware Sidebar (collapsed 64px persisted), Context panel 380px (copilot placeholder labelled 1.9), Command bar ⌘K.
- Dashboard: `GET /api/v1/dashboard` — widget descriptors + honest empties; gateway proxies workspace + dashboard.
- A11y: `pnpm test:a11y` (axe on shell / dashboard / members / login structures); also `pnpm test:unit` for table virtualisation smoke.

When adding a primitive: export from `packages/design-system/src/index.ts`, document props/keyboard/a11y in `packages/design-system/README.md`, and show it on `/dev/components` when practical.

## Definition of done (9 gates)

Every PR must clear the checklist in `.github/PULL_REQUEST_TEMPLATE.md`.

Auth-specific DoD tests (see `services/core/tests/auth_phase11.rs`):

- Refresh replay after rotation revokes the family
- Membership revocation invalidates sessions within 10s
- Brute-force → lockout (not 500s)
- Org A token cannot read org B
- MFA required for Owner/Admin

## Toolchain

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm typecheck
pnpm lint
pnpm check:openapi-drift
pnpm test:a11y
pnpm test:unit
```

CI: `.github/workflows/ci.yml` (also `workflow_dispatch`). Postgres non-superuser + `pg_trgm` + Redis. No live `AI_API_KEY` required.

## Invariants

Read [docs/00-INDEX.md](docs/00-INDEX.md) before proposing architecture changes.
