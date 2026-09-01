# Degradation ladder (TRD 8.2) — simulated game days

Simulated in CI (`cargo test` game-day modules). **Not** a live multi-region drill.

| Dependency | Expected degraded behaviour | Proven by |
|------------|-----------------------------|-----------|
| OpenSearch down | Query falls back to Postgres `search_doc_mirror` list; response `degraded=true` + search banner | `companyos-search` game day test |
| ClickHouse down | Dashboards/queries use Postgres mirror; freshness shows staleness + `clickhouse_degraded` | `companyos-analytics` game day test |
| AI provider down | Copilot returns `feature_disabled`; other modules continue | `companyos-ai` game day test |
| NATS down | Domain writes succeed; unpublished `outbox_event` accumulates | `companyos-outbox` / hello isolation + game day |
| Temporal down | Initiating actions persist intent; start deferred; no data loss | ApprovalProcess temporal helpers + game day |
| Redis down | Rate-limit → in-memory; SSE → poll; authz + idempotency via DB | Gateway rate-limit fallback + workflow idempotency tests |

See also [`gaps.md`](./gaps.md) for honest incomplete paths.
