# ADR 015: Region as tenant attribute from v1

- Status: **Accepted**
- Date: 2026-03-25
- Updated: 2026-09-01 (Phase 4.1 foundations)

## Context

CompanyOS Phase 0 must lock foundational platform decisions before product domains land.

## Decision

Each org carries a region attribute from day one to enable later data residency without retrofit.

Phase 4.1 makes this concrete:

- Column `organization.region` (`us` | `eu` | `ap`), set at creation, **immutable** by default
- Region catalogue + residency policy in `crates/tenancy::region` (not in `crates/authz`)
- Access JWT carries `region`; gateway binds to a cell and rejects cross-region data-plane
- Object keys are `{region}/org/{org_uuid}/…`
- Failover only to in-region standby when policy allows (`us` → `us-dr`; EU/AP fail closed)

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
- Contract: `docs/compliance/data-residency.md`.
- Runbook: `docs/runbooks/regional-failover.md`.
