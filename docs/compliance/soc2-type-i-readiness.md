# SOC 2 Type I readiness (in-repo)

Status: **Readiness pack** — not a paid auditor letter or Type I attestation.

CompanyOS ships control evidence collection points an auditor would ask for during
a Type I readiness engagement. Controls map to Trust Services Criteria (security)
with pointers into product surfaces and CI-provable gates.

## Evidence collection points

| Control theme | Product evidence | How to collect / CI proof |
|---------------|------------------|---------------------------|
| Access control (CC6) | Access review who-could / who-did + CSV/JSON export | `GET /api/v1/governance/access-review/...`; `governance_phase26` |
| Audit integrity (CC7) | Hash-chained `audit_entry` + verify job | `POST /api/v1/governance/audit/verify`; tamper fixture |
| Logical access / SSO (CC6) | Enterprise OIDC SSO configs + login tests | Dual mocked IdP CI (`sso_phase26`) — not live Okta/Entra |
| Tenant isolation (CC6) | Postgres RLS (non-superuser) | `hello_isolation`, tenancy suite, restore drill |
| Encryption at rest (CC6.6) | Field blobs + CMEK MockKms wrap/rotate | `companyos-crypto` + `phase42_enterprise` |
| SCIM provisioning (CC6) | SCIM 2.0 Users/Groups + two mock IdP tokens | `phase42_enterprise` |
| Retention (CC6 / A1) | Per-org retention config + dry-run cutoff | Retention dry-run test |
| Change management | Git history + outbox governance events | Event schemas + PR review |
| Secrets hygiene | Hashed API keys + JWKS/MockKms rotation | `docs/runbooks/secret-rotation.md`; `scripts/ops/rotate-test-secret.sh` |
| Backup / restore (A1) | Postgres dump → isolated restore + RLS + journal balance | `ops-restore-drill` CI job |
| Monitoring / response | Alert catalogue ↔ runbook link check | `ops-alert-runbooks` CI job |
| Availability degradation | TRD 8.2 game days (simulated) | Game-day tests + [`docs/ops/degradation-ladder.md`](../ops/degradation-ladder.md) |

## Control catalogue → tests (index)

See [`control-mapping.md`](./control-mapping.md) for TSC rows. Quick map:

| Control | Primary tests |
|---------|---------------|
| RLS isolation | `crates/testkit`, `hello_isolation`, restore drill |
| Audit hash-chain | `governance_phase26` |
| Access reviews | `governance_phase26` |
| Encryption-at-rest fields | HR field perms + `companyos-crypto` |
| SSO/OIDC mocks | `sso_phase26` |
| Backup restore drill | `scripts/ops/restore-drill.sh` |
| CMEK / SCIM mocks | `phase42_enterprise` |

## Out of scope (explicit)

- Paid SOC 2 Type I / Type II attestation letter
- Live AWS KMS / PrivateLink / multi-region production cells
- Live Okta + Azure AD in CI (mocked IdPs only)
- External penetration test (attack-surface appendix prepared for later)
- 30-day 99.9% availability claims, design-partner / TTFV metrics
- Store-signed iOS/Android

## Related

- Control mapping: [`control-mapping.md`](./control-mapping.md)
- DPIA template: [`dpia-template.md`](./dpia-template.md)
- Sub-processors: [`sub-processors.md`](./sub-processors.md)
- Ops gaps: [`../ops/gaps.md`](../ops/gaps.md)
- Threat model refresh: [`../threat-models/platform-ops.md`](../threat-models/platform-ops.md)
