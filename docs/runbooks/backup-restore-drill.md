# Runbook: Backup restore drill

## Purpose

A **restore drill counts as a backup**. CI runs
`scripts/ops/restore-drill.sh` against an isolated Postgres database to prove:

1. `pg_dump` → `pg_restore` into a separate database succeeds
2. RLS still holds for a **non-superuser** role (`NOSUPERUSER NOBYPASSRLS`)
3. A flagship invariant holds after restore: journal **debit = credit**

## Targets (not claimed production numbers)

| Metric | Target |
|--------|--------|
| RPO | ≤ 5 minutes |
| RTO | ≤ 60 minutes |

See [`docs/ops/rpo-rto-targets.md`](../ops/rpo-rto-targets.md).

## CI

Job `ops-restore-drill` in `.github/workflows/ci.yml` (required).

## Manual / staging

```bash
export PGHOST=127.0.0.1 PGPORT=5432 PGUSER=postgres PGPASSWORD=postgres
./scripts/ops/restore-drill.sh
```

## Pass criteria

- Script exits 0
- Non-superuser cannot read cross-tenant rows after restore
- Seeded journal entry balances (`SUM(debit_minor) = SUM(credit_minor)`)
