# DPIA template (Data Protection Impact Assessment)

Use this template before enabling high-risk processing (payroll figures,
government IDs, bank details, SSO identity links, long retention, SCIM sync,
CMEK key custody, mobile offline caches).

## 1. Processing description

| Field | Value |
|-------|-------|
| Feature / change | |
| Data categories | e.g. compensation, government ID, bank, payroll figures, SSO subject |
| Data subjects | Employees, contractors, org admins, end customers (CRM) |
| Purpose | |
| Legal basis (GDPR Art. 6/9) | |
| Retention | Default days + overrides (`org_retention_config`) |
| Sub-processors involved | See [`sub-processors.md`](./sub-processors.md) |
| Regions / residency | `organization.region` (ADR-015); cell routing |

## 2. Necessity & proportionality

- Why is this processing needed for the stated purpose?
- Can the same outcome be achieved with less data or shorter retention?
- Field-level permissions in use (`hr.field.*`, `finance.field.*`)?
- Is offline mobile/desktop cache minimized and encrypted at rest on device?

## 3. Risks to individuals

| Risk | Likelihood | Impact | Mitigations |
|------|------------|--------|-------------|
| Unauthorized sensitive read | | | PDP + field perms + audit |
| Audit tampering | | | Hash chain + verify job |
| Over-retention | | | Retention dry-run + workflow hard-delete |
| SSO / SCIM misconfiguration | | | Enterprise plan/feature flag; mock IdP tests in CI |
| Backup restore leakage | | | RLS forced; restore drill asserts isolation |
| CMEK misuse / revoke | | | Dual-control rotate; MockKms revoke fail-closed |
| Cross-region residency breach | | | Region gate + residency tests |

## 4. Consultation

- DPO / privacy counsel:
- Security review:
- Residual risk accepted by:

## 5. Sign-off

| Role | Name | Date |
|------|------|------|
| Product | | |
| Security | | |
| Legal / DPO | | |
