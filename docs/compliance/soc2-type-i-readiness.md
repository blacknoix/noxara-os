# SOC 2 Type I readiness (in-repo)

Status: **Readiness pack** — not a paid auditor letter or Type I attestation.

CompanyOS Phase 2.6 ships the control evidence collection points an auditor would
ask for during a Type I readiness engagement. Controls map to Trust Services
Criteria (security) with pointers into product surfaces.

## Evidence collection points

| Control theme | Product evidence | How to collect |
|---------------|------------------|----------------|
| Access control (CC6) | Access review who-could / who-did + CSV/JSON export | `GET /api/v1/governance/access-review/...`, UI `/settings/security` |
| Audit integrity (CC7) | Hash-chained `audit_entry` + verify job | `POST /api/v1/governance/audit/verify` |
| Logical access / SSO (CC6) | Enterprise OIDC SSO configs + login tests | `/api/v1/auth/sso/...`, dual mocked IdP CI |
| Retention (CC6 / A1) | Per-org retention config + dry-run cutoff | `GET|PUT /api/v1/governance/retention`, dry-run |
| Change management | Git history + outbox governance events | `admin.access_review.completed`, `admin.retention.changed`, `auth.sso.linked` |
| Secrets hygiene | Hashed API keys + rotation | `POST /api/v1/governance/api-keys/{id}/rotate` |
| Field-level privacy | HR/Finance field permissions | Catalogue `hr.field.*`, `finance.field.*` |

## Out of scope (explicit)

- Paid SOC 2 Type I / Type II attestation letter
- Customer-managed encryption keys (CMEK)
- Private connectivity / VPC peering product
- SCIM provisioning (Phase 4)
- Live Okta + Azure AD in CI (mocked IdPs only)

## Related

- Control mapping: [`control-mapping.md`](./control-mapping.md)
- DPIA template: [`dpia-template.md`](./dpia-template.md)
- Sub-processors: [`sub-processors.md`](./sub-processors.md)
