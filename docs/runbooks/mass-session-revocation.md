# Runbook: Mass session revocation

## When

Suspected refresh-token theft, insider threat, or post-incident containment for one user or an entire org.

## Per user (self-service)

```bash
curl -X DELETE "$GATEWAY_URL/api/v1/auth/sessions" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H "Cookie: companyos_refresh=..."
```

Or revoke a single device from the avatar → Sessions menu in web.

## Per user in an org (admin)

Revoke membership (also bumps `policy_version` and revokes org sessions):

```bash
curl -X POST "$GATEWAY_URL/api/v1/auth/memberships/$USER_PUBLIC_OR_UUID/revoke" \
  -H "Authorization: Bearer $OWNER_ACCESS_TOKEN"
```

Access tokens for that membership fail live checks within seconds (DoD: &lt; 10s).

## Org-wide emergency

1. Rotate JWKS (see auth-key-rotation runbook) to invalidate minting with leaked secrets.
2. SQL (break-glass, on-call only):

```sql
UPDATE auth_session SET revoked_at = now(), revoke_reason = 'mass_revoke'
WHERE org_id = '<org-uuid>' AND revoked_at IS NULL;
UPDATE refresh_token SET revoked_at = now()
WHERE session_id IN (SELECT id FROM auth_session WHERE org_id = '<org-uuid>');
```

3. Notify owners; require password reset + MFA re-confirm for privileged roles.
