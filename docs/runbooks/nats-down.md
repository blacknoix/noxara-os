# NATS down

## Meaning

Outbox relay cannot publish to JetStream (`NATS_URL`). Domain **writes continue**;
`outbox_event` unpublished rows accumulate until NATS recovers.

## Check

```bash
# Health / monitoring port (compose maps 8222)
curl -sf http://127.0.0.1:8222/healthz || echo "NATS unhealthy"

# Bootstrap streams if missing
scripts/nats-bootstrap.sh
```

```sql
SELECT COUNT(*) FROM outbox_event WHERE published_at IS NULL;
```

## Remediation

1. Restore NATS / JetStream
2. Ensure `COMPANYOS_EVENTS` + DLQ streams exist
3. Restart `companyos-outbox-relay`
4. Replay DLQ if needed ([dlq-depth](./dlq-depth.md))

## Game day expectation

Writes succeed with NATS unreachable; unpublished count increases (CI game day).
