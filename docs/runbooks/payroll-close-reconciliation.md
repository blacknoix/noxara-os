# Payroll close / reconciliation

## Meaning

A payroll run moved to paid (or close checklist) but Finance journal lines do not
balance, or payroll journal uniqueness / source linkage looks wrong.

## Constraints

- Payroll journal unique is **payroll-only** — do not restore a broad unique on
  `(org_id, source_type, source_id)`.
- Money is integer minor units.

## Check

```sql
SELECT e.public_id, e.source_type, e.source_id,
       SUM(l.debit_minor) AS debit, SUM(l.credit_minor) AS credit
FROM finance_journal_entry e
JOIN finance_journal_line l ON l.entry_id = e.id AND l.org_id = e.org_id
WHERE e.org_id = $org AND e.source_type = 'payroll'
GROUP BY e.public_id, e.source_type, e.source_id
HAVING SUM(l.debit_minor) <> SUM(l.credit_minor);
```

Confirm payroll run status vs journal source id in People / Finance UIs.

## Remediation

1. Do **not** edit posted lines — post a correcting balanced journal if policy allows
2. Re-run payroll calculate/approve path only for draft runs
3. Escalate finance owner if paid-run cash has already left the building
4. See also [failed-payment-reconciliation](./failed-payment-reconciliation.md)
