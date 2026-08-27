# Runbook: local development

## Start

```bash
cp .env.example .env   # once
make dev-up
```

Starts Docker Compose services needed for the hello/auth/CRM/finance path (**Postgres, Redis, NATS JetStream, Temporal, MinIO**), runs migrations + seed (Acme Demo + OrgProvisioning), and launches:

| Process | Port |
|---|---|
| gateway | 8080 |
| core | 8081 |
| crm | 8082 |
| finance | 8083 |
| project (+ worker) | 8084 |
| hr / people | 8088 |
| notification | 8085 |
| search | 8086 |
| analytics | 8087 |
| file | 8089 |
| outbox-relay | 8090 |
| workflow-host | 8091 |
| ai (mock unless `AI_API_KEY`) | 8092 |

Then:

```bash
pnpm install
pnpm --filter @companyos/web dev
```

Optional analytics/search backends (not required for deal-to-cash):

```bash
docker compose -f infrastructure/docker/docker-compose.yml --profile full up -d
```

Hello / CRM / finance work if OpenSearch/ClickHouse are down.

## Stop

```bash
make dev-down
```

## Auth (Phase 1.1)

Primary: org-scoped access JWT + `companyos_refresh` httpOnly cookie.

- Web routes: `/login`, `/signup`, `/verify-email`, `/magic-link`, `/mfa`, `/reset-password`
- Mail links log to the console and `.tmp/mail/`
- Seeded member: `member@acme.demo` / `correct-horse-battery`
- Seeded owner requires MFA enrollment after password login
- Share `AUTH_JWT_SECRET` between core and gateway (`dev-up` injects the same env into every process)
- `COMPANYOS_LOCAL_AUTH=1` re-enables Phase 0 header/unsigned bypass (default **off** in `.env.example` and `dev-up`)

See also: [auth threat model](../threat-models/auth.md), ADRs 016, runbooks for key rotation / mass revoke / locked-out owner, and [deal-to-cash](deal-to-cash.md).

## PostgreSQL RLS and the `companyos` role

The compose bootstrap user is `postgres` (superuser). Init creates a separate
`companyos` login with `NOSUPERUSER NOBYPASSRLS` — app and tests must use that role.
Superusers bypass RLS even with `FORCE ROW LEVEL SECURITY`. `companyos-testkit::connect()`
fails loudly if the connected role still bypasses RLS.

`pg_trgm` is created as superuser on both `companyos` and `companyos_test` (CRM near-name duplicate detection).
