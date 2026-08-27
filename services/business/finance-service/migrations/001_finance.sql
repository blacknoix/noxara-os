-- Phase 1.5 Finance schema. Tenant isolation via org_id + FORCE RLS.
-- Issued invoices, credit notes, and journal entries are immutable
-- (enforced by triggers). Gapless invoice numbers via finance_invoice_seq.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ---------------------------------------------------------------------------
-- Customer projection (from Sales events — never FK into sales_* tables)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_customer (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    sales_customer_public_id TEXT NOT NULL,
    name            TEXT NOT NULL,
    email           TEXT,
    currency        CHAR(3) NOT NULL DEFAULT 'USD',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, sales_customer_public_id)
);
CREATE INDEX IF NOT EXISTS finance_customer_org_idx ON finance_customer (org_id);
ALTER TABLE finance_customer ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_customer FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_customer_tenant_isolation ON finance_customer
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Tax rates / groups
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_tax_rate (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    rate_bps        BIGINT NOT NULL CHECK (rate_bps >= 0),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_tax_rate_org_idx ON finance_tax_rate (org_id);
ALTER TABLE finance_tax_rate ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_tax_rate FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_tax_rate_tenant_isolation ON finance_tax_rate
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Ledger chart of accounts (seeded per org)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_ledger_account (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    code            TEXT NOT NULL,
    name            TEXT NOT NULL,
    account_type    TEXT NOT NULL CHECK (account_type IN (
        'asset', 'liability', 'equity', 'revenue', 'expense'
    )),
    normal_balance  TEXT NOT NULL CHECK (normal_balance IN ('debit', 'credit')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, code)
);
CREATE INDEX IF NOT EXISTS finance_ledger_account_org_idx ON finance_ledger_account (org_id);
ALTER TABLE finance_ledger_account ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_ledger_account FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_ledger_account_tenant_isolation ON finance_ledger_account
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Gapless invoice numbering (transactional next-number)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_invoice_seq (
    org_id          UUID NOT NULL REFERENCES organization(id),
    year            INT NOT NULL,
    next_number     BIGINT NOT NULL DEFAULT 1 CHECK (next_number >= 1),
    PRIMARY KEY (org_id, year)
);
ALTER TABLE finance_invoice_seq ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_invoice_seq FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_invoice_seq_tenant_isolation ON finance_invoice_seq
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Invoices
-- status: draft → issued → sent → partially_paid → paid → overdue → void
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_invoice (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    customer_id     UUID NOT NULL REFERENCES finance_customer(id),
    owner_user_id   UUID NOT NULL,
    status          TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'issued', 'sent', 'partially_paid', 'paid', 'overdue', 'void'
    )),
    invoice_number  TEXT,
    currency        CHAR(3) NOT NULL,
    base_currency   CHAR(3) NOT NULL DEFAULT 'USD',
    fx_rate_num     BIGINT,
    fx_rate_den     BIGINT,
    fx_rate_date    DATE,
    subtotal_minor  BIGINT NOT NULL DEFAULT 0,
    discount_minor  BIGINT NOT NULL DEFAULT 0,
    tax_minor       BIGINT NOT NULL DEFAULT 0,
    total_minor     BIGINT NOT NULL DEFAULT 0,
    base_total_minor BIGINT NOT NULL DEFAULT 0,
    amount_paid_minor BIGINT NOT NULL DEFAULT 0,
    amount_credited_minor BIGINT NOT NULL DEFAULT 0,
    balance_minor   BIGINT NOT NULL DEFAULT 0,
    issue_date      DATE,
    due_date        DATE,
    sent_at         TIMESTAMPTZ,
    paid_at         TIMESTAMPTZ,
    voided_at       TIMESTAMPTZ,
    source_quote_public_id TEXT,
    source_quote_snapshot JSONB,
    payment_url     TEXT,
    notes           TEXT,
    terms           TEXT,
    version         INT NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, invoice_number)
);
CREATE INDEX IF NOT EXISTS finance_invoice_org_idx ON finance_invoice (org_id);
CREATE INDEX IF NOT EXISTS finance_invoice_customer_idx ON finance_invoice (org_id, customer_id);
CREATE INDEX IF NOT EXISTS finance_invoice_status_idx ON finance_invoice (org_id, status);
ALTER TABLE finance_invoice ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_invoice FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_invoice_tenant_isolation ON finance_invoice
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_invoice_line (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    invoice_id      UUID NOT NULL REFERENCES finance_invoice(id) ON DELETE CASCADE,
    public_id       TEXT NOT NULL,
    position        INT NOT NULL DEFAULT 0,
    description     TEXT NOT NULL,
    quantity        BIGINT NOT NULL DEFAULT 1,
    unit_price_minor BIGINT NOT NULL DEFAULT 0,
    discount_minor  BIGINT NOT NULL DEFAULT 0,
    tax_rate_bps    BIGINT NOT NULL DEFAULT 0,
    tax_minor       BIGINT NOT NULL DEFAULT 0,
    line_total_minor BIGINT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_invoice_line_inv_idx ON finance_invoice_line (invoice_id);
ALTER TABLE finance_invoice_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_invoice_line FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_invoice_line_tenant_isolation ON finance_invoice_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Credit notes (immutable once issued)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_credit_note (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    invoice_id      UUID NOT NULL REFERENCES finance_invoice(id),
    customer_id     UUID NOT NULL REFERENCES finance_customer(id),
    owner_user_id   UUID NOT NULL,
    status          TEXT NOT NULL DEFAULT 'issued' CHECK (status IN ('issued', 'void')),
    credit_number   TEXT NOT NULL,
    currency        CHAR(3) NOT NULL,
    subtotal_minor  BIGINT NOT NULL DEFAULT 0,
    tax_minor       BIGINT NOT NULL DEFAULT 0,
    total_minor     BIGINT NOT NULL DEFAULT 0,
    reason          TEXT,
    issued_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, credit_number)
);
CREATE INDEX IF NOT EXISTS finance_credit_note_org_idx ON finance_credit_note (org_id);
ALTER TABLE finance_credit_note ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_credit_note FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_credit_note_tenant_isolation ON finance_credit_note
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_credit_note_line (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    credit_note_id  UUID NOT NULL REFERENCES finance_credit_note(id) ON DELETE CASCADE,
    description     TEXT NOT NULL,
    quantity        BIGINT NOT NULL DEFAULT 1,
    unit_price_minor BIGINT NOT NULL DEFAULT 0,
    tax_rate_bps    BIGINT NOT NULL DEFAULT 0,
    tax_minor       BIGINT NOT NULL DEFAULT 0,
    line_total_minor BIGINT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE finance_credit_note_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_credit_note_line FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_credit_note_line_tenant_isolation ON finance_credit_note_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Payments + allocations
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_payment (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    customer_id     UUID NOT NULL REFERENCES finance_customer(id),
    owner_user_id   UUID NOT NULL,
    currency        CHAR(3) NOT NULL,
    amount_minor    BIGINT NOT NULL CHECK (amount_minor > 0),
    amount_allocated_minor BIGINT NOT NULL DEFAULT 0,
    amount_unapplied_minor BIGINT NOT NULL,
    method          TEXT NOT NULL DEFAULT 'manual' CHECK (method IN (
        'manual', 'stripe_webhook', 'other'
    )),
    provider        TEXT,
    provider_event_id TEXT,
    received_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS finance_payment_provider_event_uidx
    ON finance_payment (org_id, provider, provider_event_id)
    WHERE provider_event_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS finance_payment_org_idx ON finance_payment (org_id);
ALTER TABLE finance_payment ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_payment FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_payment_tenant_isolation ON finance_payment
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_payment_allocation (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    payment_id      UUID NOT NULL REFERENCES finance_payment(id),
    invoice_id      UUID NOT NULL REFERENCES finance_invoice(id),
    amount_minor    BIGINT NOT NULL CHECK (amount_minor > 0),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS finance_payment_alloc_pay_idx ON finance_payment_allocation (payment_id);
CREATE INDEX IF NOT EXISTS finance_payment_alloc_inv_idx ON finance_payment_allocation (invoice_id);
ALTER TABLE finance_payment_allocation ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_payment_allocation FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_payment_allocation_tenant_isolation ON finance_payment_allocation
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Expenses
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_expense_category (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    code            TEXT NOT NULL,
    name            TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, code)
);
ALTER TABLE finance_expense_category ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_expense_category FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_expense_category_tenant_isolation ON finance_expense_category
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_expense (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    owner_user_id   UUID NOT NULL,
    category_id     UUID REFERENCES finance_expense_category(id),
    status          TEXT NOT NULL DEFAULT 'submitted' CHECK (status IN (
        'submitted', 'pending_approval', 'approved', 'rejected', 'posted'
    )),
    currency        CHAR(3) NOT NULL,
    amount_minor    BIGINT NOT NULL CHECK (amount_minor > 0),
    description     TEXT NOT NULL,
    receipt_url     TEXT,
    receipt_meta    JSONB,
    incurred_at     DATE NOT NULL DEFAULT CURRENT_DATE,
    decided_by      UUID,
    decided_at      TIMESTAMPTZ,
    decision_note   TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_expense_org_idx ON finance_expense (org_id);
ALTER TABLE finance_expense ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_expense FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_expense_tenant_isolation ON finance_expense
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Journal (append-only double-entry)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_journal_entry (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    entry_date      DATE NOT NULL DEFAULT CURRENT_DATE,
    memo            TEXT NOT NULL,
    source_type     TEXT NOT NULL CHECK (source_type IN (
        'invoice_issue', 'payment', 'credit_note', 'expense', 'manual'
    )),
    source_id       UUID,
    currency        CHAR(3) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_journal_entry_org_idx ON finance_journal_entry (org_id);
ALTER TABLE finance_journal_entry ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_journal_entry FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_journal_entry_tenant_isolation ON finance_journal_entry
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_journal_line (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    entry_id        UUID NOT NULL REFERENCES finance_journal_entry(id),
    account_id      UUID NOT NULL REFERENCES finance_ledger_account(id),
    debit_minor     BIGINT NOT NULL DEFAULT 0 CHECK (debit_minor >= 0),
    credit_minor    BIGINT NOT NULL DEFAULT 0 CHECK (credit_minor >= 0),
    memo            TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (debit_minor > 0 AND credit_minor = 0)
        OR (credit_minor > 0 AND debit_minor = 0)
    )
);
CREATE INDEX IF NOT EXISTS finance_journal_line_entry_idx ON finance_journal_line (entry_id);
ALTER TABLE finance_journal_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_journal_line FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_journal_line_tenant_isolation ON finance_journal_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Recurring invoices (schema + scheduler hook; full Temporal later)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_recurring_invoice (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    customer_id     UUID NOT NULL REFERENCES finance_customer(id),
    owner_user_id   UUID NOT NULL,
    cadence         TEXT NOT NULL CHECK (cadence IN ('monthly', 'quarterly', 'yearly')),
    next_run_at     TIMESTAMPTZ NOT NULL,
    active          BOOLEAN NOT NULL DEFAULT true,
    template        JSONB NOT NULL,
    last_invoice_id UUID REFERENCES finance_invoice(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
ALTER TABLE finance_recurring_invoice ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_recurring_invoice FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_recurring_invoice_tenant_isolation ON finance_recurring_invoice
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Idempotency + provider webhook inbox
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_idempotency (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    scope           TEXT NOT NULL,
    key             TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body   JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, scope, key)
);
ALTER TABLE finance_idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_idempotency FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_idempotency_tenant_isolation ON finance_idempotency
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS finance_webhook_event (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    provider        TEXT NOT NULL,
    event_id        TEXT NOT NULL,
    event_type      TEXT NOT NULL,
    payload         JSONB NOT NULL,
    processed_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, provider, event_id)
);
ALTER TABLE finance_webhook_event ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_webhook_event FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS finance_webhook_event_tenant_isolation ON finance_webhook_event
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Immutability triggers: issued invoices, credit notes, journal
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION finance_reject_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'finance immutability: % is append-only / issued documents cannot be mutated', TG_TABLE_NAME
        USING ERRCODE = 'integrity_constraint_violation';
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION finance_invoice_guard() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.status <> 'draft' THEN
            RAISE EXCEPTION 'finance immutability: cannot delete non-draft invoice'
                USING ERRCODE = 'integrity_constraint_violation';
        END IF;
        RETURN OLD;
    END IF;
    -- UPDATE: draft may change freely; non-draft only status/payment fields
    IF OLD.status <> 'draft' THEN
        IF NEW.public_id IS DISTINCT FROM OLD.public_id
            OR NEW.customer_id IS DISTINCT FROM OLD.customer_id
            OR NEW.invoice_number IS DISTINCT FROM OLD.invoice_number
            OR NEW.currency IS DISTINCT FROM OLD.currency
            OR NEW.subtotal_minor IS DISTINCT FROM OLD.subtotal_minor
            OR NEW.discount_minor IS DISTINCT FROM OLD.discount_minor
            OR NEW.tax_minor IS DISTINCT FROM OLD.tax_minor
            OR NEW.total_minor IS DISTINCT FROM OLD.total_minor
            OR NEW.base_total_minor IS DISTINCT FROM OLD.base_total_minor
            OR NEW.fx_rate_num IS DISTINCT FROM OLD.fx_rate_num
            OR NEW.fx_rate_den IS DISTINCT FROM OLD.fx_rate_den
            OR NEW.source_quote_public_id IS DISTINCT FROM OLD.source_quote_public_id
        THEN
            RAISE EXCEPTION 'finance immutability: cannot mutate issued invoice document fields'
                USING ERRCODE = 'integrity_constraint_violation';
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS finance_invoice_immutability ON finance_invoice;
CREATE TRIGGER finance_invoice_immutability
    BEFORE UPDATE OR DELETE ON finance_invoice
    FOR EACH ROW EXECUTE FUNCTION finance_invoice_guard();

CREATE OR REPLACE FUNCTION finance_invoice_line_guard() RETURNS trigger AS $$
DECLARE
    inv_status TEXT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        SELECT status INTO inv_status FROM finance_invoice WHERE id = OLD.invoice_id;
        IF inv_status IS NOT NULL AND inv_status <> 'draft' THEN
            RAISE EXCEPTION 'finance immutability: cannot delete lines of issued invoice'
                USING ERRCODE = 'integrity_constraint_violation';
        END IF;
        RETURN OLD;
    END IF;
    SELECT status INTO inv_status FROM finance_invoice WHERE id = NEW.invoice_id;
    IF inv_status IS NOT NULL AND inv_status <> 'draft' THEN
        RAISE EXCEPTION 'finance immutability: cannot mutate lines of issued invoice'
            USING ERRCODE = 'integrity_constraint_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS finance_invoice_line_immutability ON finance_invoice_line;
CREATE TRIGGER finance_invoice_line_immutability
    BEFORE INSERT OR UPDATE OR DELETE ON finance_invoice_line
    FOR EACH ROW EXECUTE FUNCTION finance_invoice_line_guard();

DROP TRIGGER IF EXISTS finance_credit_note_immutability ON finance_credit_note;
CREATE TRIGGER finance_credit_note_immutability
    BEFORE UPDATE OR DELETE ON finance_credit_note
    FOR EACH ROW EXECUTE FUNCTION finance_reject_mutation();

DROP TRIGGER IF EXISTS finance_credit_note_line_immutability ON finance_credit_note_line;
CREATE TRIGGER finance_credit_note_line_immutability
    BEFORE UPDATE OR DELETE ON finance_credit_note_line
    FOR EACH ROW EXECUTE FUNCTION finance_reject_mutation();

DROP TRIGGER IF EXISTS finance_journal_entry_immutability ON finance_journal_entry;
CREATE TRIGGER finance_journal_entry_immutability
    BEFORE UPDATE OR DELETE ON finance_journal_entry
    FOR EACH ROW EXECUTE FUNCTION finance_reject_mutation();

DROP TRIGGER IF EXISTS finance_journal_line_immutability ON finance_journal_line;
CREATE TRIGGER finance_journal_line_immutability
    BEFORE UPDATE OR DELETE ON finance_journal_line
    FOR EACH ROW EXECUTE FUNCTION finance_reject_mutation();

-- Allow INSERT of lines only when creating a credit note (no UPDATE/DELETE).
-- Journal lines: INSERT only (append).
