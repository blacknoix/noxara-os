-- Phase 2.3: payroll journal source type + wage accounts (extends 1.5 CoA seeds).
-- Full CoA / month-end is Phase 2.4 — these are minimal posting primitives.

ALTER TABLE finance_journal_entry DROP CONSTRAINT IF EXISTS finance_journal_entry_source_type_check;
ALTER TABLE finance_journal_entry ADD CONSTRAINT finance_journal_entry_source_type_check
    CHECK (source_type IN (
        'invoice_issue', 'payment', 'credit_note', 'expense', 'manual', 'payroll'
    ));

-- One journal per payroll run (idempotent pay). Must NOT cover source_type=payment:
-- record-then-allocate posts a second payment journal for the same payment_id.
DROP INDEX IF EXISTS finance_journal_entry_org_source_uidx;
CREATE UNIQUE INDEX IF NOT EXISTS finance_journal_entry_org_source_payroll_uidx
    ON finance_journal_entry (org_id, source_type, source_id)
    WHERE source_type = 'payroll' AND source_id IS NOT NULL;
