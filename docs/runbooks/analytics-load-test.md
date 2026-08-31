# Runbook: analytics load testing

## Purpose and budgets

Use this runbook to validate the Phase 3.2 governed analytics path without
loading operational tables directly. Measure end-to-end API latency separately
from warehouse ingestion latency.

| Workload                                                      | p95 budget |
| ------------------------------------------------------------- | ---------: |
| Metric catalogue, freshness, and cached benchmark reads       |     400 ms |
| Interactive report/dashboard queries (up to 500 grouped rows) |        2 s |
| Forecast, CSV export, and scheduled report fire               |        5 s |

Record error rate, p50, p95, p99, concurrency, fixture size, and the commit SHA.
A p95 result is valid only when the error rate is below 1% and permission or
validation failures are reported separately from server errors.

## Storage paths

Facts enter analytics only through domain events and
`POST /api/v1/analytics/internal/ingest`.

- CI and the default local profile use the PostgreSQL analytics schema. Every
  fact/config table carries `org_id`, uses RLS, and every ad-hoc query requires
  an explicit public `org_id`.
- The `full` Compose profile starts ClickHouse. DDL is versioned in
  `services/platform/analytics-service/clickhouse/`.
- With `CLICKHOUSE_URL` set, ingest writes the PostgreSQL CI mirror and sends a
  best-effort copy to ClickHouse. The Phase 3.2 API query executor still reads
  the PostgreSQL mirror; benchmark ClickHouse directly as described below when
  validating the warehouse path.

Never seed analytics by copying invoice, deal, or task OLTP rows. Produce the
same event envelopes that the outbox consumers receive.

## CI fixture

The deterministic fixture lives in
`services/platform/analytics-service/tests/phase32_analytics.rs`. It registers
an owner through core, provisions system roles, creates a Member token, and
ingests invoice-lifecycle and deal-stage events. It covers the query guard,
permission-filtered rows, tenant isolation, dry-run behavior, forecasts,
schedules, and CSV export.

Run it against a non-superuser, non-`BYPASSRLS` PostgreSQL role:

```bash
export TEST_DATABASE_URL='postgres://companyos:companyos@127.0.0.1:5432/companyos_test'
cargo test -p companyos-analytics --test phase32_analytics -- --nocapture
cargo test -p companyos-analytics --test phase18_analytics -- --nocapture
```

When `TEST_DATABASE_URL` and `DATABASE_URL` are both absent, the DB-backed tests
return early by design; the metric-catalogue golden test still runs.

For a larger repeatable load fixture, replay copies of the two event envelope
shapes with new UUIDv7 event IDs, stable public record IDs, and a controlled
number of organizations. Keep the distribution and random seed with the test
result. Include at least:

- invoice `issued`, `paid`, and `voided` lifecycle events;
- deal `stage_changed`, `won`, and `lost` events;
- currencies and stage dimensions with both common and long-tail values;
- a Member viewer that can run analytics but cannot read invoice facts; and
- a second organization used to assert zero cross-tenant records.

## Start the full ClickHouse profile

```bash
docker compose -f infrastructure/docker/docker-compose.yml --profile full up -d clickhouse
curl --fail --silent --show-error \
  'http://127.0.0.1:8123/?multiquery=1' \
  --data-binary @services/platform/analytics-service/clickhouse/001_fact_invoice_issued.sql
curl --fail --silent --show-error \
  'http://127.0.0.1:8123/?multiquery=1' \
  --data-binary @services/platform/analytics-service/clickhouse/002_phase32_facts.sql
export CLICKHOUSE_URL='http://127.0.0.1:8123'
make dev-up
```

Confirm both stores receive a newly ingested fixture before collecting latency:

```bash
curl --fail --silent \
  'http://127.0.0.1:8123/?query=SELECT%20count()%20FROM%20fact_invoice_lifecycle'
psql "$DATABASE_URL" -c \
  "SELECT count(*) FROM analytics_fact_invoice_lifecycle;"
```

The PostgreSQL count requires an RLS-scoped application session. A zero count
from an unscoped non-superuser session is expected; do not disable RLS to make
the check pass.

## Run API load

Obtain an owner access token, organization public ID, and saved report public
ID through the normal core/analytics APIs. Keep credentials out of shell
history where possible.

The examples use `oha`; an equivalent HTTP load generator is acceptable.

```bash
read -rsp 'Access token: ' ACCESS_TOKEN
echo
export ORG_ID='org_...'
export REPORT_ID='rpt_...'

oha -z 60s -c 20 \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  "http://127.0.0.1:8080/api/v1/analytics/benchmarks?org_id=$ORG_ID"

oha -z 60s -c 10 -m POST \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{}' \
  "http://127.0.0.1:8080/api/v1/analytics/reports/$REPORT_ID/run"
```

Run exports and schedule fires at lower concurrency because they create run and
outbox records:

```bash
oha -n 100 -c 2 -m POST \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"format":"csv"}' \
  "http://127.0.0.1:8080/api/v1/analytics/reports/$REPORT_ID/export"
```

Use a dedicated test organization and database. Do not run write-heavy export
or schedule workloads against production.

## Benchmark ClickHouse

Use the same `org_id` predicate required by the API query guard. Convert the
organization public ID to its UUID when preparing the fixture and keep it in
`ORG_UUID`.

```bash
export ORG_UUID='00000000-0000-0000-0000-000000000000'
oha -z 60s -c 20 \
  "http://127.0.0.1:8123/?query=SELECT%20currency%2Csum(amount_minor)%20FROM%20fact_invoice_lifecycle%20WHERE%20org_id%3D%27$ORG_UUID%27%20AND%20lifecycle_event%3D%27issued%27%20GROUP%20BY%20currency"
```

Reject any benchmark query that omits `org_id`, even when it is only a local
test. Compare direct ClickHouse p95 with API p95 to separate warehouse cost
from authentication, authorization, serialization, and run-audit overhead.

## Troubleshooting

- `401`/`403`: refresh the token and verify the role has both
  `analytics.report.run` and the source metric's read permission.
- Empty rows with `permission_denied_empty=true`: expected for a viewer that
  may run the report but cannot read its source module.
- PostgreSQL rows present but ClickHouse empty: check `CLICKHOUSE_URL`, the
  analytics log, DDL table names, and best-effort insert warnings.
- High report p95 with fast direct SQL: inspect authorization DB calls, run
  audit inserts, outbox writes, response row count, and drill-link expansion.
- Cross-tenant data: stop the test immediately, preserve the query and request
  ID, and treat it as an RLS/query-guard incident rather than a performance
  failure.
