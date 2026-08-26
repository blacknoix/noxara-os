-- Phase 1.7: quote discount holds via Operations approval engine.
ALTER TABLE sales_quote DROP CONSTRAINT IF EXISTS sales_quote_status_check;
ALTER TABLE sales_quote
    ADD CONSTRAINT sales_quote_status_check
    CHECK (status IN (
        'draft', 'sent', 'accepted', 'rejected', 'expired', 'pending_approval'
    ));

ALTER TABLE sales_quote
    ADD COLUMN IF NOT EXISTS approval_id TEXT;
ALTER TABLE sales_quote
    ADD COLUMN IF NOT EXISTS discount_approval_threshold_bps INT;

CREATE INDEX IF NOT EXISTS sales_quote_approval_idx
    ON sales_quote (org_id, approval_id)
    WHERE approval_id IS NOT NULL;
