# SLO burn

## Meaning

Error-budget or latency-budget burn is elevated (gateway 5xx rate, p95 read/write
budgets). CI load harness records **budgets** (read ≤200 ms / write ≤400 ms) against
local services — not production SLOs.

## Check

- Gateway / service RED metrics (when exporters configured)
- Recent deploy / migration / dependency outage correlation
- `scripts/ops/load-harness.sh` artifact in CI (informational job)

## Remediation

1. Identify top failing route / dependency from logs
2. Roll back or feature-flag if burn is deploy-correlated
3. Engage degradation ladder ([docs/ops/degradation-ladder.md](../ops/degradation-ladder.md))
4. Page on-call only when staging/prod monitors fire (not CI budgets)
