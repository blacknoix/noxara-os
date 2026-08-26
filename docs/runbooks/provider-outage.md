# Runbook: Payment provider outage

## When

Stripe-like webhooks are delayed, duplicated, or the provider API is unreachable. Live provider keys are **not** required for local/dev; the webhook endpoint is fixture-driven behind env.

## Steps

1. Confirm finance service health: `GET http://127.0.0.1:8083/healthz` (via gateway `/api/v1/finance/...` when proxied).
2. Check env:
   - `FINANCE_WEBHOOK_SECRET` — shared secret for unsigned fixture posts
   - `FINANCE_SERVICE_URL` / `FINANCE_BIND`
   - Never store card data; payloads should only carry amounts, currency, customer/invoice ids, and provider event ids.
3. During outage, record cash manually:

```http
POST /api/v1/finance/payments
Idempotency-Key: …
{ "customer_id": "cus_…", "currency": "USD", "amount_minor": 10000, "invoice_id": "inv_…" }
```

4. When webhooks resume, replay is safe: identical `(org_id, provider, event_id)` returns `{ "duplicate": true }` without creating a second payment.
5. Out-of-order events: each distinct `event_id` is processed independently; allocation still respects remaining invoice balance.

## Verify

- Fixture replay test / manual POST twice with the same `id` → one `finance_payment` row.
- Journal remains balanced; unapplied remainder sits in Customer Credits (2200).
