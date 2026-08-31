-- Phase 3.5 — Finance depth: versioned tax rates, dunning profiles, multi-entity.
-- Append-only tax rates (validity windows). No DROP POLICY under FORCE RLS.

-- ---------------------------------------------------------------------------
-- Tax groups
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_tax_group (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_tax_group_org_idx ON finance_tax_group (org_id);
ALTER TABLE finance_tax_group ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_tax_group FORCE ROW LEVEL SECURITY;
CREATE POLICY finance_tax_group_tenant_isolation ON finance_tax_group
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Extend finance_tax_rate with validity windows + group linkage (append-only)
-- ---------------------------------------------------------------------------
ALTER TABLE finance_tax_rate
    ADD COLUMN IF NOT EXISTS valid_from DATE NOT NULL DEFAULT '1970-01-01',
    ADD COLUMN IF NOT EXISTS valid_to DATE,
    ADD COLUMN IF NOT EXISTS tax_group_id UUID,
    ADD COLUMN IF NOT EXISTS supersedes_id UUID,
    ADD COLUMN IF NOT EXISTS component_name TEXT,
    ADD COLUMN IF NOT EXISTS is_component BOOLEAN NOT NULL DEFAULT false;

-- FKs added separately so IF NOT EXISTS column adds stay safe on re-run.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_tax_rate_tax_group_id_fkey'
    ) THEN
        ALTER TABLE finance_tax_rate
            ADD CONSTRAINT finance_tax_rate_tax_group_id_fkey
            FOREIGN KEY (tax_group_id) REFERENCES finance_tax_group(id);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_tax_rate_supersedes_id_fkey'
    ) THEN
        ALTER TABLE finance_tax_rate
            ADD CONSTRAINT finance_tax_rate_supersedes_id_fkey
            FOREIGN KEY (supersedes_id) REFERENCES finance_tax_rate(id);
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS finance_tax_rate_group_valid_idx
    ON finance_tax_rate (org_id, tax_group_id, valid_from);

-- Forbid in-place mutation of rate_bps (create a new version instead).
CREATE OR REPLACE FUNCTION finance_tax_rate_forbid_rate_bps_change() RETURNS trigger AS $$
BEGIN
    IF NEW.rate_bps IS DISTINCT FROM OLD.rate_bps THEN
        RAISE EXCEPTION 'finance_tax_rate.rate_bps is immutable; create a new version';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS finance_tax_rate_rate_bps_immutable ON finance_tax_rate;
CREATE TRIGGER finance_tax_rate_rate_bps_immutable
    BEFORE UPDATE ON finance_tax_rate
    FOR EACH ROW
    EXECUTE FUNCTION finance_tax_rate_forbid_rate_bps_change();

-- ---------------------------------------------------------------------------
-- Finance entities (multi-entity foundations — NOT consolidation)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_entity (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    code            TEXT NOT NULL,
    currency        CHAR(3) NOT NULL DEFAULT 'USD',
    is_default      BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, code)
);
CREATE INDEX IF NOT EXISTS finance_entity_org_idx ON finance_entity (org_id);
CREATE UNIQUE INDEX IF NOT EXISTS finance_entity_org_default_uidx
    ON finance_entity (org_id) WHERE is_default = true;
ALTER TABLE finance_entity ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_entity FORCE ROW LEVEL SECURITY;
CREATE POLICY finance_entity_tenant_isolation ON finance_entity
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Dunning profiles
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS finance_dunning_profile (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    is_default      BOOLEAN NOT NULL DEFAULT false,
    steps           JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS finance_dunning_profile_org_idx ON finance_dunning_profile (org_id);
CREATE UNIQUE INDEX IF NOT EXISTS finance_dunning_profile_org_default_uidx
    ON finance_dunning_profile (org_id) WHERE is_default = true;
ALTER TABLE finance_dunning_profile ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_dunning_profile FORCE ROW LEVEL SECURITY;
CREATE POLICY finance_dunning_profile_tenant_isolation ON finance_dunning_profile
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Stamp invoices / lines / journals / customers
-- ---------------------------------------------------------------------------
ALTER TABLE finance_invoice
    ADD COLUMN IF NOT EXISTS entity_id UUID,
    ADD COLUMN IF NOT EXISTS tax_rate_id UUID;

ALTER TABLE finance_invoice_line
    ADD COLUMN IF NOT EXISTS tax_rate_id UUID,
    ADD COLUMN IF NOT EXISTS tax_group_id UUID;

ALTER TABLE finance_journal_entry
    ADD COLUMN IF NOT EXISTS entity_id UUID;

ALTER TABLE finance_customer
    ADD COLUMN IF NOT EXISTS dunning_profile_id UUID,
    ADD COLUMN IF NOT EXISTS billing_entity_id UUID;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_invoice_entity_id_fkey'
    ) THEN
        ALTER TABLE finance_invoice
            ADD CONSTRAINT finance_invoice_entity_id_fkey
            FOREIGN KEY (entity_id) REFERENCES finance_entity(id);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_invoice_tax_rate_id_fkey'
    ) THEN
        ALTER TABLE finance_invoice
            ADD CONSTRAINT finance_invoice_tax_rate_id_fkey
            FOREIGN KEY (tax_rate_id) REFERENCES finance_tax_rate(id);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_invoice_line_tax_rate_id_fkey'
    ) THEN
        ALTER TABLE finance_invoice_line
            ADD CONSTRAINT finance_invoice_line_tax_rate_id_fkey
            FOREIGN KEY (tax_rate_id) REFERENCES finance_tax_rate(id);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_invoice_line_tax_group_id_fkey'
    ) THEN
        ALTER TABLE finance_invoice_line
            ADD CONSTRAINT finance_invoice_line_tax_group_id_fkey
            FOREIGN KEY (tax_group_id) REFERENCES finance_tax_group(id);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_journal_entry_entity_id_fkey'
    ) THEN
        ALTER TABLE finance_journal_entry
            ADD CONSTRAINT finance_journal_entry_entity_id_fkey
            FOREIGN KEY (entity_id) REFERENCES finance_entity(id);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_customer_dunning_profile_id_fkey'
    ) THEN
        ALTER TABLE finance_customer
            ADD CONSTRAINT finance_customer_dunning_profile_id_fkey
            FOREIGN KEY (dunning_profile_id) REFERENCES finance_dunning_profile(id);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'finance_customer_billing_entity_id_fkey'
    ) THEN
        ALTER TABLE finance_customer
            ADD CONSTRAINT finance_customer_billing_entity_id_fkey
            FOREIGN KEY (billing_entity_id) REFERENCES finance_entity(id);
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS finance_invoice_entity_idx
    ON finance_invoice (org_id, entity_id);
