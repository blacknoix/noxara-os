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
