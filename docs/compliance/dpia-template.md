# DPIA template (Data Protection Impact Assessment)

Use this template before enabling high-risk processing (payroll figures,
government IDs, bank details, SSO identity links, long retention).

## 1. Processing description

| Field | Value |
|-------|-------|
| Feature / change | |
| Data categories | e.g. compensation, government ID, bank, payroll figures |
| Data subjects | Employees, contractors, org admins |
| Purpose | |
| Legal basis (GDPR Art. 6/9) | |
| Retention | Default days + overrides (`org_retention_config`) |

## 2. Necessity & proportionality

- Why is this processing needed for the stated purpose?
- Can the same outcome be achieved with less data or shorter retention?
- Field-level permissions in use (`hr.field.*`, `finance.field.*`)?

## 3. Risks to individuals

| Risk | Likelihood | Impact | Mitigations |
|------|------------|--------|-------------|
| Unauthorized sensitive read | | | PDP + field perms + audit |
| Audit tampering | | | Hash chain + verify job |
| Over-retention | | | Retention dry-run + workflow hard-delete |
| SSO misconfiguration | | | Enterprise plan/feature flag gate |

## 4. Consultation

- DPO / privacy counsel:
- Security review:
- Residual risk accepted by:

## 5. Sign-off

| Role | Name | Date |
|------|------|------|
| Product | | |
| Security | | |
| Privacy | | |
