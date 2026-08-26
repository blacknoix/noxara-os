# Runbook: Stuck invoice

## When

An invoice is stuck in `draft`, `issued`, or `sent` when operators expect `paid` / `void`, or issue fails under concurrency.

## Steps

1. Load the invoice:

```http
GET /api/v1/finance/invoices/{inv_…}
```

2. If still `draft` and issue fails with conflict on number, check the sequence:

```sql
SELECT * FROM finance_invoice_seq WHERE org_id = $org;
SELECT invoice_number, status FROM finance_invoice WHERE org_id = $org ORDER BY created_at DESC LIMIT 20;
```

Issue uses a transactional `UPDATE … RETURNING next_number - 1` plus `UNIQUE (org_id, invoice_number)`.

3. If issued but payments do not clear the balance, inspect allocations (see failed-payment-reconciliation runbook). Overpayments leave `amount_unapplied_minor` and Customer Credits — that is intentional.

4. To abandon an unpaid issued invoice, use void (does not mutate document totals; status → `void`):

```http
POST /api/v1/finance/invoices/{inv_…}/void
```

Do **not** UPDATE/DELETE issued rows — immutability triggers will reject document field changes.

5. Corrections: issue a credit note (`POST /api/v1/finance/credit-notes`) rather than editing the invoice.

## Verify

- Attempting `UPDATE finance_invoice SET total_minor = … WHERE status <> 'draft'` fails at the database.
- Outbox contains `companyos.{org}.finance.invoice.issued.v1` (and paid/void events as applicable) in the same transaction as the domain write.
