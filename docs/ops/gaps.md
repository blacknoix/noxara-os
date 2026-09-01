# Ops gaps (known / honest)

This file lists degradation-ladder and ops items that are **not** fully
implemented or not proven in production. Do not treat CI greens as live SLOs.

| Gap | Status | Notes |
|-----|--------|-------|
| Durable multi-instance search mirror under OpenSearch outage | Partial | Ingest dual-writes `search_doc_mirror` (Postgres). Query falls back to mirror + `degraded` banner when OpenSearch fails. Process-local memory remains the unset-`OPENSEARCH_URL` path. |
| Live ClickHouse query path | Not shipped | Analytics **queries always hit Postgres fact mirror** (ADR-011). ClickHouse is best-effort ingest only. Freshness reports `clickhouse_degraded` when CH URL set but unreachable. |
| Live AI provider outage auto-disable | Hook + CI | `AI_PROVIDER_FORCE_DOWN=1` or provider `Err` → copilot returns `feature_disabled`; rest of app unaffected. No multi-region provider failover. |
| Live NATS outage alert wiring | Doc + CI game day | Writes already continue via outbox; relay lag/DLQ alerts documented. No paging integration in-repo. |
| Temporal SDK start while down | Deferred OK | Approval/workflow rows persist; Temporal start is best-effort and deferred. Full worker rehydration needs Temporal back. |
| Redis authz cache | N/A by design | Authz is **per-request from Postgres** (sole PDP). Redis only backs rate-limit/SSE; both fall back without Redis. |
| Production RPO/RTO measurement | Target only | See [`rpo-rto-targets.md`](./rpo-rto-targets.md). CI restore drill ≠ prod timed restore. |
| Real Okta/Entra, AWS KMS, PrivateLink, App Store | Out of scope | Mocked OIDC / MockKms / unsigned mobile only. |
| External pen test | Out of scope | Attack-surface appendix in threat model for later engagement. |
| 30-day 99.9% availability / 10 design partners / TTFV | Out of scope | Human/ops metrics. |
| 10M-row analytics p95 | Out of scope | Modest CI load harness only. |
