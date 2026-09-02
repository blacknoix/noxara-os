# DLQ depth alert

## Meaning

`outbox_dlq` rows with `replayed_at IS NULL` are growing. Events failed
publish after max relay attempts and need operator attention.

## Check

```sql
SELECT COUNT(*) AS dlq_depth
FROM outbox_dlq
WHERE replayed_at IS NULL;

SELECT id, subject, error, attempts, created_at
FROM outbox_dlq
WHERE replayed_at IS NULL
ORDER BY created_at
LIMIT 50;
```

Relay metrics: `curl -s http://127.0.0.1:8090/metrics | jq .dlq`

## Remediation

1. Fix NATS / publisher root cause ([nats-down](./nats-down.md))
2. Replay: `scripts/outbox-dlq-replay.sh --all` (or `--id <uuid>`)
3. Confirm lag drops ([outbox-lag](./outbox-lag.md))
