# Data residency contract (Phase 4.1)

Status: **contractual** for CompanyOS multi-region foundations.  
Authoritative code: `crates/tenancy/src/region.rs` (`region_catalogue()`).

## Regions

| Code | Jurisdiction | Primary cell | In-region standby | Data-plane may leave region? |
|------|--------------|--------------|-------------------|------------------------------|
| `us` | United States | `us-primary` | `us-dr` | Only to `us-dr` (DisasterRecovery) |
| `eu` | EU/EEA (GDPR) | `eu-primary` | — | **No** |
| `ap` | Asia Pacific | `ap-primary` | — | **No** |

## What may replicate globally

Deny-by-default for data-plane payloads. Explicitly allowlisted:

| Replica kind | Meaning | Regions |
|--------------|---------|---------|
| `RoutingMetadata` | Org→home-region map, cell health | all |
| `Identity` | Login, sessions, JWT minting (tokens remain org-scoped) | all |
| `DisasterRecovery` | In-region standby cell only | **us only** |

## What must not leave the home region

- CRM / finance / ops / HR / inventory rows
- Files / object storage (`{region}/org/{org_uuid}/…`)
- Search indexes and analytics facts
- NATS event payloads and Temporal workflow execution for domain work
- Any other tenant-owned data-plane store

## Org home region

- Set at organization creation (`register` / `POST /workspace/organizations`).
- Stored on `organization.region` (ADR-015).
- **Immutable** after creation (DB trigger). Change-region is an explicit Temporal migration workflow (out of Phase 4.1 default).
- Embedded in access JWT as `region` claim; gateway propagates `X-CompanyOS-Region`.

## Failover

- Mark primary unhealthy → traffic may cut over to an **in-region** standby only if residency policy allows `DisasterRecovery`.
- EU/AP fail **closed** (no US standby).
- Production RTO: full-region restore ≤ **60 minutes** (TRD).
- CI proves the drill in seconds; see `docs/runbooks/regional-failover.md`.

## Edge routing

Latency-based edge routing is documented as a Cloudflare / geo DNS rule that sets `X-CompanyOS-Region`. Gateway tests honor the header; a real Anycast PoP is **not** required for this phase.

## Evidence pack

See [`multi-region-evidence.md`](./multi-region-evidence.md) for how a reviewer validates enforcement paths later on live infra.
