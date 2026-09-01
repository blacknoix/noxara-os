# Sub-processor documentation template

List third parties that process customer personal data on behalf of the
CompanyOS operator. Update when vendors change.

## Active sub-processors

| Vendor | Purpose | Data categories | Region(s) | DPA in place | Notes |
|--------|---------|-----------------|-----------|--------------|-------|
| Cloud DB provider | Primary datastore | All tenant data | | | RLS + `org_id`; backups |
| Object storage (files) | File blobs | Uploaded files | | | Region-prefixed keys (4.1) |
| Email delivery | Auth / notifications | Email, name | | | |
| Error / metrics (optional) | Telemetry | Request metadata (no secrets) | | | Forbidden log keys |
| LLM provider (optional) | Copilot / agents | Prompt context (customer-controlled) | | | No live `AI_API_KEY` in CI; kill switch |
| Push relay (optional) | Mobile push | Device tokens | | | Fakes in CI; no live FCM/APNs yet |

## Customer-configured IdPs / KMS (not CompanyOS sub-processors)

When an enterprise customer connects their own OIDC IdP (Okta, Entra ID, …) or
CMEK, that relationship is the **customer’s**. CompanyOS stores IdP configuration
metadata, SCIM tokens, subject links, and wrapped DEKs only.

## Change log

| Date | Change | Owner |
|------|--------|-------|
| 2026-09-01 | Refresh for Phases 4.x + 1.11 + ops gates | Engineering |
| | Initial template (Phase 2.6) | |
