# SOC 2 Type I — control mapping

Maps Trust Services Criteria (Security) to CompanyOS Phase 2.6 controls.
This is an engineering readiness map, not an audit opinion.

| TSC | Control objective | Implementation | Evidence |
|-----|-------------------|----------------|----------|
| CC1.1 | Roles & responsibilities | System roles in `crates/authz`; Owner/Admin MFA | Role catalogue tests |
| CC2.1 | Communication of policies | In-repo threat models + ADRs | `docs/threat-models/auth.md` |
| CC3.1 | Risk assessment | Auth + tenancy threat models | `docs/threat-models/` |
| CC5.1 | Control activities | Sole PDP `crates/authz` deny-by-default | Catalogue CI green |
| CC6.1 | Logical access | Org-scoped JWT, membership live checks, RLS | `auth_phase11`, tenancy tests |
| CC6.2 | Credentials | Argon2id, refresh rotation, hashed API keys | API key rotate tests |
| CC6.3 | Access removal | Membership revoke + offboarding checklist | HR offboarding workflow |
| CC6.6 | Encryption / sensitive fields | Field encryption + field-level perms | HR sensitive read audits |
| CC6.7 | Transmission | TLS at gateway (prod); no secrets in logs | Telemetry redaction |
| CC7.1 | Detection | Append-only hash-chained audit partitions | `audit_verify` job |
| CC7.2 | Monitoring anomalies | Audit verify fails closed on break | Tamper fixture test |
| CC8.1 | Change management | PR review + outbox governance events | Git + event schemas |
| A1.2 | Availability / retention | Soft delete + retention config dry-run | Retention dry-run test |

## Access review Q&A (DoD)

An admin can answer **“who could see payroll in March, and who did?”** via:

1. `GET /api/v1/governance/access-review/who-could-see?permission_id=hr.payroll.read&period_start=…&period_end=…`
2. `GET /api/v1/governance/access-review/who-did?permission_id=hr.payroll.read&period_start=…&period_end=…`
3. Export: `GET /api/v1/governance/access-review/runs/{id}/export?format=csv|json`

Under two minutes from the product UI at `/settings/security`.
