-- Phase 2.5 — Vendor bills (procure-to-pay support for inventory-service).
-- AP is booked at goods-receipt time via the standard journal endpoint
-- (Dr Inventory / Cr Accounts Payable — Vendors). A vendor bill is an
-- internal record of that liability against a specific PO/GRN reference;
-- creating a bill posts NO journal (it would double-count AP already
-- booked at receipt). Paying a bill posts Dr AP / Cr Cash.

-- Extend the journal source_type allowlist (Rust-level ALLOWED_SOURCE_TYPES
-- in handlers/journals.rs) to cover the Phase 2.5 inventory + vendor-bill
-- postings: goods-receipt AP accrual, issue COGS, asset depreciation, and
-- vendor-bill payment.
ALTER TABLE finance_journal_entry DROP CONSTRAINT IF EXISTS finance_journal_entry_source_type_check;
ALTER TABLE finance_journal_entry ADD CONSTRAINT finance_journal_entry_source_type_check
    CHECK (source_type IN (
        'invoice_issue', 'payment', 'credit_note', 'expense', 'manual', 'payroll',
        'inventory_receipt', 'inventory_cogs', 'inventory_depreciation',
        'vendor_bill', 'vendor_payment'
    ));

CREATE TABLE IF NOT EXISTS finance_vendor_bill (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    supplier_ref        TEXT NOT NULL,
    source_type         TEXT NOT NULL DEFAULT 'goods_receipt' CHECK (source_type IN (
        'goods_receipt', 'purchase_order', 'manual'
    )),
    source_id           TEXT,
    currency            CHAR(3) NOT NULL,
    amount_minor        BIGINT NOT NULL CHECK (amount_minor > 0),
    amount_paid_minor   BIGINT NOT NULL DEFAULT 0 CHECK (amount_paid_minor >= 0),
    status              TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'partially_paid', 'paid', 'void')),
    memo                TEXT,
    payment_journal_public_id TEXT,
    created_by          UUID,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_vendor_bill_org_status_idx
    ON finance_vendor_bill (org_id, status);
CREATE INDEX IF NOT EXISTS finance_vendor_bill_org_source_idx
    ON finance_vendor_bill (org_id, source_type, source_id) WHERE source_id IS NOT NULL;
ALTER TABLE finance_vendor_bill ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_vendor_bill FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_vendor_bill_tenant_isolation ON finance_vendor_bill;
CREATE POLICY finance_vendor_bill_tenant_isolation ON finance_vendor_bill
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Bill create/pay idempotency reuses the existing generic `finance_idempotency`
-- table (001_finance.sql) — no separate table needed here.
