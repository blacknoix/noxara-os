# Contributing to CompanyOS (noxara-os)

## Prerequisites

- Rust stable (1.85+)
- pnpm 9+
- Docker (for `scripts/dev-up`) or local Postgres 16
- PostgreSQL client optional (`psql`) for seed scripts

## Quick start

```bash
cp .env.example .env
make dev-up
pnpm install
pnpm --filter @companyos/web dev
```

Run core + gateway with the same `AUTH_JWT_SECRET`. See root [README.md](README.md) for auth details.

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

## Invariants

Read [docs/00-INDEX.md](docs/00-INDEX.md) before proposing architecture changes.
