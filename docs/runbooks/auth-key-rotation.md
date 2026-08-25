# Runbook: Auth signing key (JWKS) rotation

## When

Suspected signing-key exposure, scheduled rotation, or after `AUTH_JWT_SECRET` change in a multi-instance deploy.

## Steps

1. Ensure core and gateway share the same key material path (`AUTH_JWT_SECRET` bootstrap + `jwks_signing_key` table).
2. As an Owner/Admin with `admin.membership.manage`, call:

```bash
curl -X POST "$GATEWAY_URL/api/v1/auth/jwks/rotate" \
  -H "Authorization: Bearer $ACCESS_TOKEN"
```

3. Confirm `GET /api/v1/auth/jwks.json` lists the new `kid` and retains prior kids for verification grace.
4. Gateway refreshes JWKS about every 60s; force restart if immediate pickup is required.
5. Existing access tokens signed with the old kid remain valid until expiry; refresh issues tokens under the new kid.
6. After the access-token TTL window, retire old rows: set `retired_at` on unused kids (ops SQL) once no verify traffic remains.

## Verify

- Login → refresh → hello still succeeds.
- `kid` on new access JWTs matches the active key.
