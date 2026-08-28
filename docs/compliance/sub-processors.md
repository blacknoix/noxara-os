# Sub-processor documentation template

List third parties that process customer personal data on behalf of the
CompanyOS operator. Update when vendors change. SCIM / marketplace IdPs are
out of scope until Phase 4.

## Active sub-processors

| Vendor | Purpose | Data categories | Region(s) | DPA in place | Notes |
|--------|---------|-----------------|-----------|--------------|-------|
| Cloud DB provider | Primary datastore | All tenant data | | | RLS + `org_id` |
| Object storage (files) | File blobs | Uploaded files | | | |
| Email delivery | Auth / notifications | Email, name | | | |
| Error / metrics (optional) | Telemetry | Request metadata (no secrets) | | | Forbidden log keys |

## Customer-configured IdPs (not CompanyOS sub-processors)

When an enterprise customer connects their own OIDC IdP (Okta, Entra ID, …),
that IdP is the **customer’s** processor/controller relationship — CompanyOS
stores only IdP configuration metadata and subject links (`sso_identity_link`).

## Change log

| Date | Change | Owner |
|------|--------|-------|
| | Initial template (Phase 2.6) | |
