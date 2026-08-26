-- Phase 1.7: link expenses to the Operations approval engine (opaque apr_ id).
ALTER TABLE finance_expense
    ADD COLUMN IF NOT EXISTS approval_id TEXT;
CREATE INDEX IF NOT EXISTS finance_expense_approval_idx
    ON finance_expense (org_id, approval_id)
    WHERE approval_id IS NOT NULL;
