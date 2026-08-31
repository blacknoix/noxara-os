# Authentication

## API keys

1. Sign in as Owner/Admin.
2. `POST /api/v1/governance/api-keys` with `Idempotency-Key` and body:

```json
{
  "name": "billing-integration",
  "scopes": ["sales.customer.read", "sales.customer.create", "finance.invoice.read", "finance.invoice.create", "finance.invoice.issue"],
  "expires_at": null
}
```

3. Store `secret` immediately — it is never re-shown.
4. Call public routes with `Authorization: Bearer <secret>` or `X-Api-Key: <secret>`.

Revoke with `POST /api/v1/governance/api-keys/{id}/revoke`. Rotate with
`POST /api/v1/governance/api-keys/{id}/rotate` (old secret stops working).

Keys are hashed at rest (`key_hash`), prefixed for display, track `last_used_at`,
support expiry, and are rate-limited **separately** from user sessions
(`RateLimit-*` + `429`/`Retry-After`).
