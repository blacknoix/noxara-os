# Multi-region enforcement evidence pack (Phase 4.1)

This pack documents how a reviewer can prove residency enforcement.  
**Live independent audit of AWS/GCP network paths is out of this PR** — ship enforcement + this checklist.

## What CI proves today

| Check | Where |
|-------|--------|
| Region catalogue + policy (US DR allowed; EU/AP deny DR) | `crates/tenancy` region unit tests |
| Object key includes `{region}/org/{org_uuid}/…`; wrong-cell get denied | `crates/tenancy` + `file-service` |
| Analytics/search reject missing or mismatched region | `analytics-service` / `search-indexer` unit + handler guards |
| EU org denied on US cell (HTTP 451 `residency_violation`) | `companyos-gateway` `region_gate` tests |
| US primary down → `us-dr` cutover within CI budget | `run_failover_drill` + control-plane drill API |
| EU primary down → fail closed (no US standby) | same |
| `organization.region` immutable | migration trigger + core tests |

## Reviewer checklist (later, live infra)

1. Confirm each regional cell’s gateway has `COMPANYOS_CELL_ID` / `COMPANYOS_CELL_REGION` set correctly.
2. Confirm object buckets are per-cell and keys are prefixed with the cell region.
3. Capture VPC / private-link diagrams showing data-plane stores are not peered cross-region except allowlisted control-plane endpoints.
4. Replay the failover runbook (`docs/runbooks/regional-failover.md`) in staging; time wall-clock RTO against 60 minutes.
5. Sample NATS subjects and Temporal workflow IDs — still `{org}`-scoped; confirm workers are cell-bound and do not poll another region’s task queues.
6. Spot-check audit rows in `region_routing_audit` for health changes and drills.

## Out of scope here

- Real AWS multi-region cells / Kubernetes multi-cluster
- CMEK, SCIM, dedicated isolation (Phase 4.2)
- Autonomous agents (Phase 4.3)
