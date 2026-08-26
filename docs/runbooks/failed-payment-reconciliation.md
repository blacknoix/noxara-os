# Runbook: Failed payment reconciliation

## When

A payment webhook was accepted (or a manual payment was recorded) but the invoice balance / journal does not match expected cash, or finance reports show unapplied credits unexpectedly.

## Steps

1. Identify the payment public id (`pay_…`) and org.
2. Confirm the webhook inbox row (idempotency):

```sql
SELECT id, provider, event_id, event_type, processed_at, created_at
FROM finance_webhook_event
WHERE org_id = $org AND provider = 'stripe'
ORDER BY created_at DESC
LIMIT 20;
```

3. Inspect payment + allocations:

```sql
SELECT p.public_id, p.amount_minor, p.amount_allocated_minor, p.amount_unapplied_minor, p.provider_event_id
FROM finance_payment p
WHERE p.org_id = $org AND p.public_id = $pay;
SELECT a.amount_minor, i.public_id AS invoice_id, i.balance_minor, i.status
FROM finance_payment_allocation a
JOIN finance_invoice i ON i.id = a.invoice_id
WHERE a.payment_id = $payment_uuid;
```

4. Confirm journal balance for the payment source:

```sql
SELECT e.public_id, e.memo, SUM(l.debit_minor) AS debit, SUM(l.credit_minor) AS credit
FROM finance_journal_entry e
JOIN finance_journal_line l ON l.entry_id = e.id
WHERE e.org_id = $org AND e.source_type = 'payment' AND e.source_id = $payment_uuid
GROUP BY e.public_id, e.memo;
```

5. If the webhook was duplicated, expect `duplicate: true` on replay and a single payment row (unique on `provider_event_id`).
6. If cash was received but not allocated, allocate via `POST /api/v1/finance/payments/{id}/allocate` (requires `finance.payment.allocate`). Do not edit journal lines.

## Verify

- Invoice `balance_minor = total_minor - amount_paid_minor - amount_credited_minor`
- Journal debit total equals credit total for the entry
- Reports summary receivables/cash move only via new postings
