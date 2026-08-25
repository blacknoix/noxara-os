# Runbook: local development

## Start

```bash
make dev-up
```

Starts Docker Compose services (Postgres, Redis, NATS JetStream, Temporal, MinIO), seeds one org + two users, and launches core + gateway.

Optional analytics/search:

```bash
docker compose -f infrastructure/docker/docker-compose.yml --profile full up -d
```

Hello works if OpenSearch/ClickHouse are down.

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
- Share `AUTH_JWT_SECRET` between core and gateway
- `COMPANYOS_LOCAL_AUTH=1` re-enables Phase 0 header/unsigned bypass (default **off**)

See also: [auth threat model](../threat-models/auth.md), ADRs 016, runbooks for key rotation / mass revoke / locked-out owner.

## PostgreSQL RLS and the `companyos` role

The compose bootstrap user is `postgres` (superuser). Init creates a separate
`companyos` login with `NOSUPERUSER NOBYPASSRLS` — app and tests must use that role.
Superusers bypass RLS even with `FORCE ROW LEVEL SECURITY`. `companyos-testkit::connect()`
fails loudly if the connected role still bypasses RLS.
