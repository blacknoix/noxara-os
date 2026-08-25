# Contributing to CompanyOS (noxara-os)

## Prerequisites

- Rust stable (1.85+)
- pnpm 9+
- Docker (for `scripts/dev-up`)
- PostgreSQL client optional (`psql`) for seed scripts

## Quick start

```bash
cp .env.example .env
make dev-up          # or: bash scripts/dev-up
pnpm install
pnpm --filter @companyos/web dev
```

Hello path works even if OpenSearch/ClickHouse are down (they use compose profile `full`).

## LOCAL-ONLY auth

Phase 0 uses **local-only** authentication:

- Headers `X-CompanyOS-Dev-Org-Id` + `X-CompanyOS-Dev-User-Id`
- Or an **unsigned** Bearer payload containing `org_id` and `sub`

Never enable this in production. Never commit secrets.

## Definition of done (9 gates)

Every PR must clear the checklist in `.github/PULL_REQUEST_TEMPLATE.md`:

1. Functionality
2. API contract
3. Tenancy
4. Authorization (`crates/authz` only)
5. Audit & events
6. Tests
7. UI slice (loading / empty / error)
8. Observability
9. Documentation

## Toolchain

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm typecheck
pnpm lint
```

## Monorepo layout

See root [README.md](../README.md).

## Invariants

Read [docs/00-INDEX.md](docs/00-INDEX.md) before proposing architecture changes. A proposal that breaks an invariant is wrong.
