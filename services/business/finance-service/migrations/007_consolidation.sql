-- Phase 4.2 — Intercompany transactions + consolidation runs.
-- Extends finance_entity (3.5); same-currency eliminations first.
-- No DROP POLICY under FORCE RLS.

-- Allow intercompany journal postings (extends 005 allowlist; do not restore
-- a broad unique on (org, source_type, source_id) — payroll unique stays payroll-only).
ALTER TABLE finance_journal_entry DROP CONSTRAINT IF EXISTS finance_journal_entry_source_type_check;
ALTER TABLE finance_journal_entry ADD CONSTRAINT finance_journal_entry_source_type_check
    CHECK (source_type IN (
        'invoice_issue', 'payment', 'credit_note', 'expense', 'manual', 'payroll',
        'inventory_receipt', 'inventory_cogs', 'inventory_depreciation',
        'vendor_bill', 'vendor_payment',
        'intercompany'
    ));

-- Intercompany due-to / due-from clearing accounts (seeded lazily by handlers).
-- Codes reserved: 1500 IC Receivable, 2500 IC Payable, 4900 IC Revenue, 5900 IC Expense.

CREATE TABLE IF NOT EXISTS finance_intercompany_txn (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    from_entity_id      UUID NOT NULL REFERENCES finance_entity(id),
    to_entity_id        UUID NOT NULL REFERENCES finance_entity(id),
    currency            CHAR(3) NOT NULL,
    amount_minor        BIGINT NOT NULL CHECK (amount_minor > 0),
    memo                TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'posted'
                        CHECK (status IN ('draft', 'posted', 'void')),
    from_journal_id     UUID REFERENCES finance_journal_entry(id),
    to_journal_id       UUID REFERENCES finance_journal_entry(id),
    posted_by           UUID,
    posted_at           TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    CHECK (from_entity_id <> to_entity_id)
);
CREATE INDEX IF NOT EXISTS finance_intercompany_txn_org_idx
    ON finance_intercompany_txn (org_id);
ALTER TABLE finance_intercompany_txn ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_intercompany_txn FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY finance_intercompany_txn_tenant_isolation ON finance_intercompany_txn
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

CREATE TABLE IF NOT EXISTS finance_consolidation_run (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    currency            CHAR(3) NOT NULL,
    as_of               DATE NOT NULL,
    status              TEXT NOT NULL DEFAULT 'completed'
                        CHECK (status IN ('pending', 'completed', 'failed')),
    entity_ids          UUID[] NOT NULL,
    eliminated_minor    BIGINT NOT NULL DEFAULT 0,
    statements          JSONB NOT NULL DEFAULT '{}',
    created_by          UUID,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_consolidation_run_org_idx
    ON finance_consolidation_run (org_id);
ALTER TABLE finance_consolidation_run ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_consolidation_run FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY finance_consolidation_run_tenant_isolation ON finance_consolidation_run
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- Entity access membership (opaque user_id — no cross-context membership join).
CREATE TABLE IF NOT EXISTS finance_entity_access (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    entity_id       UUID NOT NULL REFERENCES finance_entity(id),
    user_id         UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, entity_id, user_id)
);
CREATE INDEX IF NOT EXISTS finance_entity_access_user_idx
    ON finance_entity_access (org_id, user_id);
ALTER TABLE finance_entity_access ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_entity_access FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY finance_entity_access_tenant_isolation ON finance_entity_access
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
