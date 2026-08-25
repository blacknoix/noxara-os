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
```

## Invariants

Read [docs/00-INDEX.md](docs/00-INDEX.md) before proposing architecture changes.
