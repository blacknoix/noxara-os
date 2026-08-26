# Outbox lag alert

## Meaning

The outbox relay (`companyos-outbox-relay`) publishes unpublished `outbox_event`
rows to NATS JetStream. When the count of unpublished rows exceeds
`OUTBOX_LAG_ALERT_THRESHOLD` (default **1000**), the relay logs:

```text
OUTBOX_LAG_ALERT: unpublished outbox events exceed threshold — see docs/runbooks/outbox-lag.md
```

High lag means domain writes succeeded but consumers (notifications, search,
analytics) are behind or NATS/publish is failing.

## Check metrics

Relay HTTP (default bind `0.0.0.0:8090`):

```bash
curl -s http://127.0.0.1:8090/metrics | jq
```

Expect fields such as `published`, `dlq`, `lag`, `batches` (and optionally
oldest unpublished timestamp).

SQL:

```sql
SELECT COUNT(*) FROM outbox_event WHERE published_at IS NULL;
SELECT COUNT(*) FROM outbox_dlq WHERE replayed_at IS NULL;
```

Relay sets `app.outbox_relay=1` for cross-tenant reads; use the relay role or
session for those counts under RLS.

## Common causes

1. `companyos-outbox-relay` not running
2. NATS down / stream missing — run `scripts/nats-bootstrap.sh`
3. Publish errors filling `outbox_dlq`
4. Extremely high write rate vs poll interval (`OUTBOX_RELAY_POLL_MS`)

## Replay DLQ

After fixing the underlying publish failure:

```bash
# Replay every unreplayed DLQ row
scripts/outbox-dlq-replay.sh --all

# Or a single row
scripts/outbox-dlq-replay.sh --id <dlq-uuid>
```

Replayed rows are re-inserted as unpublished outbox events; the relay will
publish them on the next tick.

## Embedded relay (dev only)

`OUTBOX_EMBEDDED_RELAY=1` spawns an in-process `MemoryPublisher` in core/CRM/
Finance/project. It does **not** replace production NATS publishing. Prefer the
dedicated binary in shared environments.
