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

Superusers (and roles with `BYPASSRLS`) bypass RLS even with `FORCE ROW LEVEL SECURITY`.
Compose init (`infrastructure/docker/init/01-databases.sql`) and CI demote `companyos` to
`NOSUPERUSER NOBYPASSRLS`. `companyos-testkit::connect()` fails loudly if the role still
bypasses RLS, so isolation tests cannot pass as false greens.

