-- Phase 2.4 — CoA depth, fiscal periods, bank reconciliation, expense policy.
-- Deepens Finance (does not replace invoices/payments/expenses v1).
-- Payroll unique index remains payroll-only (see 003_payroll_journal.sql).

-- ---------------------------------------------------------------------------
-- Chart of accounts: public ids, hierarchy, activation
-- ---------------------------------------------------------------------------
ALTER TABLE finance_ledger_account
    ADD COLUMN IF NOT EXISTS public_id TEXT,
    ADD COLUMN IF NOT EXISTS parent_id UUID REFERENCES finance_ledger_account(id),
    ADD COLUMN IF NOT EXISTS is_active BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS description TEXT,
    ADD COLUMN IF NOT EXISTS sort_order INT NOT NULL DEFAULT 0;

-- Backfill public_id for any pre-existing rows (idempotent).
UPDATE finance_ledger_account
SET public_id = 'acc_' || id::text
WHERE public_id IS NULL;

ALTER TABLE finance_ledger_account
    ALTER COLUMN public_id SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS finance_ledger_account_org_public_uidx
    ON finance_ledger_account (org_id, public_id);

-- Accept "income" as alias of revenue (UI language); keep revenue for ADR 019.
ALTER TABLE finance_ledger_account DROP CONSTRAINT IF EXISTS finance_ledger_account_account_type_check;
ALTER TABLE finance_ledger_account ADD CONSTRAINT finance_ledger_account_account_type_check
    CHECK (account_type IN ('asset', 'liability', 'equity', 'revenue', 'income', 'expense'));

-- ---------------------------------------------------------------------------
-- Fiscal periods (open / closed / locked)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_fiscal_period (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    code            TEXT NOT NULL,
    name            TEXT NOT NULL,
    start_date      DATE NOT NULL,
    end_date        DATE NOT NULL,
    status          TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed', 'locked')),
    closed_at       TIMESTAMPTZ,
    closed_by       UUID,
    reopened_at     TIMESTAMPTZ,
    reopened_by     UUID,
    reopen_reason   TEXT,
    checklist       JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, code),
    CHECK (end_date >= start_date)
);
CREATE INDEX IF NOT EXISTS finance_fiscal_period_org_dates_idx
    ON finance_fiscal_period (org_id, start_date, end_date);
ALTER TABLE finance_fiscal_period ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_fiscal_period FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_fiscal_period_tenant_isolation ON finance_fiscal_period;
CREATE POLICY finance_fiscal_period_tenant_isolation ON finance_fiscal_period
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Link journal entries to periods (nullable for legacy rows until backfilled).
ALTER TABLE finance_journal_entry
    ADD COLUMN IF NOT EXISTS period_id UUID REFERENCES finance_fiscal_period(id),
    ADD COLUMN IF NOT EXISTS reverses_entry_id UUID REFERENCES finance_journal_entry(id),
    ADD COLUMN IF NOT EXISTS posted_by UUID;

CREATE INDEX IF NOT EXISTS finance_journal_entry_period_idx
    ON finance_journal_entry (org_id, period_id);

-- ---------------------------------------------------------------------------
-- Bank accounts + statements + reconciliation
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_bank_account (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    name                TEXT NOT NULL,
    currency            CHAR(3) NOT NULL,
    ledger_account_id   UUID NOT NULL REFERENCES finance_ledger_account(id),
    account_number_mask TEXT,
    institution         TEXT,
    is_active           BOOLEAN NOT NULL DEFAULT true,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_bank_account_org_idx ON finance_bank_account (org_id);
ALTER TABLE finance_bank_account ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_bank_account FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_bank_account_tenant_isolation ON finance_bank_account;
CREATE POLICY finance_bank_account_tenant_isolation ON finance_bank_account
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_bank_statement (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    bank_account_id UUID NOT NULL REFERENCES finance_bank_account(id),
    statement_date  DATE NOT NULL,
    currency        CHAR(3) NOT NULL,
    opening_minor   BIGINT NOT NULL DEFAULT 0,
    closing_minor   BIGINT NOT NULL DEFAULT 0,
    source          TEXT NOT NULL DEFAULT 'csv' CHECK (source IN ('csv', 'manual')),
    import_batch_key TEXT,
    line_count      INT NOT NULL DEFAULT 0,
    created_by      UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, import_batch_key)
);
CREATE INDEX IF NOT EXISTS finance_bank_statement_org_idx ON finance_bank_statement (org_id);
ALTER TABLE finance_bank_statement ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_bank_statement FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_bank_statement_tenant_isolation ON finance_bank_statement;
CREATE POLICY finance_bank_statement_tenant_isolation ON finance_bank_statement
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_bank_statement_line (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    statement_id    UUID NOT NULL REFERENCES finance_bank_statement(id),
    line_no         INT NOT NULL,
    txn_date        DATE NOT NULL,
    amount_minor    BIGINT NOT NULL,
    currency        CHAR(3) NOT NULL,
    reference       TEXT,
    description     TEXT,
    status          TEXT NOT NULL DEFAULT 'unmatched' CHECK (status IN (
        'unmatched', 'matched', 'ignored'
    )),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (statement_id, line_no)
);
CREATE INDEX IF NOT EXISTS finance_bank_statement_line_org_idx
    ON finance_bank_statement_line (org_id, status);
ALTER TABLE finance_bank_statement_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_bank_statement_line FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_bank_statement_line_tenant_isolation ON finance_bank_statement_line;
CREATE POLICY finance_bank_statement_line_tenant_isolation ON finance_bank_statement_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_bank_reconciliation (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    bank_account_id     UUID NOT NULL REFERENCES finance_bank_account(id),
    statement_line_id   UUID NOT NULL REFERENCES finance_bank_statement_line(id),
    match_kind          TEXT NOT NULL CHECK (match_kind IN (
        'payment', 'journal_line', 'expense', 'manual'
    )),
    matched_payment_id  UUID REFERENCES finance_payment(id),
    matched_journal_line_id UUID REFERENCES finance_journal_line(id),
    matched_expense_id  UUID REFERENCES finance_expense(id),
    amount_minor        BIGINT NOT NULL,
    auto_matched        BOOLEAN NOT NULL DEFAULT false,
    matched_by          UUID,
    matched_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (statement_line_id)
);
CREATE INDEX IF NOT EXISTS finance_bank_reconciliation_org_idx ON finance_bank_reconciliation (org_id);
ALTER TABLE finance_bank_reconciliation ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_bank_reconciliation FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_bank_reconciliation_tenant_isolation ON finance_bank_reconciliation;
CREATE POLICY finance_bank_reconciliation_tenant_isolation ON finance_bank_reconciliation
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Expense policy, mileage / per-diem, card import, reimbursement batches
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_expense_policy (
    id                          UUID PRIMARY KEY,
    org_id                      UUID NOT NULL REFERENCES organization(id),
    public_id                   TEXT NOT NULL,
    name                        TEXT NOT NULL DEFAULT 'Default',
    is_active                   BOOLEAN NOT NULL DEFAULT true,
    require_receipt_over_minor  BIGINT NOT NULL DEFAULT 0 CHECK (require_receipt_over_minor >= 0),
    auto_approve_under_minor    BIGINT NOT NULL DEFAULT 0 CHECK (auto_approve_under_minor >= 0),
    over_limit_action           TEXT NOT NULL DEFAULT 'require_approval' CHECK (over_limit_action IN (
        'require_approval', 'reject'
    )),
    mileage_unit                TEXT NOT NULL DEFAULT 'mile' CHECK (mileage_unit IN ('mile', 'km')),
    mileage_rate_minor          BIGINT NOT NULL DEFAULT 0 CHECK (mileage_rate_minor >= 0),
    per_diem_minor              BIGINT NOT NULL DEFAULT 0 CHECK (per_diem_minor >= 0),
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS finance_expense_policy_one_active_uidx
    ON finance_expense_policy (org_id) WHERE is_active;
ALTER TABLE finance_expense_policy ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_expense_policy FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_expense_policy_tenant_isolation ON finance_expense_policy;
CREATE POLICY finance_expense_policy_tenant_isolation ON finance_expense_policy
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_expense_category_limit (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    policy_id       UUID NOT NULL REFERENCES finance_expense_policy(id) ON DELETE CASCADE,
    category_id     UUID NOT NULL REFERENCES finance_expense_category(id),
    max_amount_minor BIGINT NOT NULL CHECK (max_amount_minor > 0),
    currency        CHAR(3) NOT NULL DEFAULT 'USD',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (policy_id, category_id)
);
ALTER TABLE finance_expense_category_limit ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_expense_category_limit FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_expense_category_limit_tenant_isolation ON finance_expense_category_limit;
CREATE POLICY finance_expense_category_limit_tenant_isolation ON finance_expense_category_limit
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

ALTER TABLE finance_expense
    ADD COLUMN IF NOT EXISTS expense_kind TEXT NOT NULL DEFAULT 'standard'
        CHECK (expense_kind IN ('standard', 'mileage', 'per_diem', 'card')),
    ADD COLUMN IF NOT EXISTS miles_or_km NUMERIC(12, 3),
    ADD COLUMN IF NOT EXISTS per_diem_days INT,
    ADD COLUMN IF NOT EXISTS card_txn_id UUID,
    ADD COLUMN IF NOT EXISTS reimbursement_batch_id UUID;

CREATE TABLE IF NOT EXISTS finance_card_transaction (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    txn_date        DATE NOT NULL,
    amount_minor    BIGINT NOT NULL,
    currency        CHAR(3) NOT NULL,
    merchant        TEXT,
    reference       TEXT,
    description     TEXT,
    status          TEXT NOT NULL DEFAULT 'unmatched' CHECK (status IN (
        'unmatched', 'matched', 'ignored'
    )),
    matched_expense_id UUID REFERENCES finance_expense(id),
    import_batch_key TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_card_transaction_org_idx ON finance_card_transaction (org_id, status);
ALTER TABLE finance_card_transaction ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_card_transaction FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_card_transaction_tenant_isolation ON finance_card_transaction;
CREATE POLICY finance_card_transaction_tenant_isolation ON finance_card_transaction
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Deferred FK from expense → card txn (table created above).
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_expense_card_txn_fk'
    ) THEN
        ALTER TABLE finance_expense
            ADD CONSTRAINT finance_expense_card_txn_fk
            FOREIGN KEY (card_txn_id) REFERENCES finance_card_transaction(id);
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS finance_reimbursement_batch (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'pending_approval', 'approved', 'rejected', 'paid'
    )),
    currency        CHAR(3) NOT NULL,
    total_minor     BIGINT NOT NULL DEFAULT 0 CHECK (total_minor >= 0),
    owner_user_id   UUID NOT NULL,
    approval_id     TEXT,
    decided_by      UUID,
    decided_at      TIMESTAMPTZ,
    decision_note   TEXT,
    paid_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_reimbursement_batch_org_idx ON finance_reimbursement_batch (org_id);
ALTER TABLE finance_reimbursement_batch ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_reimbursement_batch FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS finance_reimbursement_batch_tenant_isolation ON finance_reimbursement_batch;
CREATE POLICY finance_reimbursement_batch_tenant_isolation ON finance_reimbursement_batch
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_expense_reimb_batch_fk'
    ) THEN
        ALTER TABLE finance_expense
            ADD CONSTRAINT finance_expense_reimb_batch_fk
            FOREIGN KEY (reimbursement_batch_id) REFERENCES finance_reimbursement_batch(id);
    END IF;
END $$;