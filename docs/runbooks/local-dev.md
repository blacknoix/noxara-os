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

## Auth

LOCAL-ONLY headers — see `.tmp/seed.env` after seed.

## PostgreSQL RLS and the `companyos` role

The compose bootstrap user is `postgres` (superuser). Init creates a separate
`companyos` login with `NOSUPERUSER NOBYPASSRLS` — app and tests must use that role.
Superusers bypass RLS even with `FORCE ROW LEVEL SECURITY`. `companyos-testkit::connect()`
fails loudly if the connected role still bypasses RLS.

