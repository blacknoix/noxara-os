# RPO / RTO targets (engineering)

These are **targets**, not claimed production measurements.

| Metric | Target | How CI proves readiness |
|--------|--------|-------------------------|
| RPO (Postgres) | ≤ 5 minutes | Continuous WAL / dump cadence in prod design; restore drill restores a recent dump and asserts invariants |
| RTO (Postgres restore) | ≤ 60 minutes | Restore drill script + regional failover drill (`crates/tenancy` CI budget); wall-clock prod timing is ops |

Related:

- Restore drill runbook: [`../runbooks/backup-restore-drill.md`](../runbooks/backup-restore-drill.md)
- Regional failover: [`../runbooks/regional-failover.md`](../runbooks/regional-failover.md)
- Gaps: [`gaps.md`](./gaps.md)
