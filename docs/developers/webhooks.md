# Outbound webhooks

Register an HTTPS endpoint that receives CompanyOS domain events.

## Create

```http
POST /api/v1/governance/webhooks
Content-Type: application/json

{
  "url": "https://example.com/hooks/companyos",
  "description": "ERP sync",
  "event_types": ["finance.invoice.issued", "sales.customer.created"]
}
```

Store `secret` once (`whsec_…`). Rotate with `…/rotate`; disable with `…/disable`.

## Signature

Every delivery includes:

```
X-CompanyOS-Signature: t={unix_seconds},v1={hex_hmac_sha256}
X-CompanyOS-Event-Id: {uuid}
X-CompanyOS-Delivery-Id: {whd_…}
```

Signed payload is `{t}.{raw_body}`. Reject if `|now - t| > 300` seconds.
Verify with HMAC-SHA256 using the endpoint secret. Plaintext secrets are never logged.

## Delivery semantics

- **At-least-once** with retries and exponential backoff
- Idempotent `event_id` per endpoint — receivers should dedupe
- Delivery log: attempt, status_code, bounded response, `next_retry_at`
- Auto-pause after repeated failures (`admin.webhook.write` to re-enable via rotate)
- SSRF protection: localhost / RFC1918 / link-local / cloud metadata URLs are rejected (fail closed)

## Replay

`POST /api/v1/governance/webhooks/deliveries/{id}/replay` re-enqueues a logged
delivery (`admin.webhook.replay`).
