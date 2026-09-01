# Regional failover runbook (Phase 4.1)

## Objectives

- Restore data-plane service for tenants whose **home region** matches a failed cell.
- Never move EU/AP tenant data to a US cell.
- Meet production RTO: **≤ 60 minutes** for full-region restore (TRD).

## Cell model (compose / CI)

| Cell | Region | Role |
|------|--------|------|
| `us-primary` | us | Primary |
| `us-dr` | us | In-region standby (allowed) |
| `eu-primary` | eu | Primary (no standby) |
| `ap-primary` | ap | Primary (no standby) |

Simulation binds a process with `COMPANYOS_CELL_ID` (gateway / file-service).  
Control-plane health lives at `/api/v1/control-plane/cells` (session auth; **not** on the public API-key allowlist).

## Drill (CI / local)

1. Register two orgs: one `us`, one `eu`.
2. `PUT /api/v1/control-plane/cells/us-primary/health` with `{"health":"unhealthy"}`.
3. `POST /api/v1/control-plane/failover-drill` with `{"org_id":"<us-org>","fail_cell":"us-primary"}`.
   - Expect `success: true`, `serving_cell: us-dr`, `within_budget: true` (CI budget = 5s).
4. Same drill for the EU org with `fail_cell: eu-primary`.
   - Expect `success: false` (fail closed — no in-region standby).
5. Confirm data-plane proxy from a US-bound gateway still returns **451** for the EU org.

Map: CI seconds ↔ production steps (DNS/geo cutover, promote DR DB, drain primary, verify health) that sum to ≤ 60 minutes.

## Production outline (not automated here)

1. Declare region incident; page on-call.
2. Mark primary cell unhealthy in the global control plane.
3. If residency allows DR: promote standby (DB, object store, workers), flip edge routing / cell VIP.
4. Verify `/readyz` on standby + sample tenant read/write.
5. Audit `region_routing_audit`; communicate status.
6. If no in-region standby: fail closed; communicate residency-preserving outage; restore primary.

## Do not

- Fail EU/AP tenants over to US (or any other region).
- Start Phase 4.2 CMEK/SCIM work from this runbook.
- Execute another region’s Temporal workflows from a recovered cell.
