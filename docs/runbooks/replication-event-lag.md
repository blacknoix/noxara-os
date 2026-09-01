# Replication / event lag

## Meaning

Downstream consumers (analytics, search, notifications) are behind the outbox /
event stream. Users may see stale dashboards or missing search hits.

## Check

```bash
curl -s "$GATEWAY_URL/api/v1/analytics/freshness?org_id=$ORG_PUBLIC_ID" \
  -H "Authorization: Bearer $TOKEN" | jq
```

Expect `lag_seconds`, `eventually_consistent`, and (when CH configured)
`clickhouse_degraded`.

```sql
SELECT COUNT(*) FROM outbox_event WHERE published_at IS NULL;
SELECT last_event_at, last_ingest_at, lag_seconds
FROM analytics_freshness WHERE org_id = $org;
```

## Remediation

1. Clear outbox / NATS blockage ([outbox-lag](./outbox-lag.md), [nats-down](./nats-down.md))
2. Confirm analytics/search ingest workers are running
3. If OpenSearch is down, search falls back to Postgres mirror with a degraded banner
4. Dashboards continue on Postgres fact mirror when ClickHouse is down
