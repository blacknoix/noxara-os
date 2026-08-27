-- Phase 1.4 CRM / Sales bounded context — customer mastered here (ADR 009).
-- Finance/invoice tables live in a separate bounded context (Phase 1.5+).

CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- ---------------------------------------------------------------------------
-- sales_pipeline
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_pipeline (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS sales_pipeline_org_id_idx ON sales_pipeline (org_id);

ALTER TABLE sales_pipeline ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_pipeline FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_pipeline_tenant_isolation ON sales_pipeline
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_pipeline_stage
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_pipeline_stage (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    pipeline_id UUID NOT NULL REFERENCES sales_pipeline(id),
    name TEXT NOT NULL,
    position INT NOT NULL DEFAULT 0,
    probability INT NOT NULL DEFAULT 0 CHECK (probability BETWEEN 0 AND 100),
    is_won BOOLEAN NOT NULL DEFAULT false,
    is_lost BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS sales_pipeline_stage_org_id_idx ON sales_pipeline_stage (org_id);
CREATE INDEX IF NOT EXISTS sales_pipeline_stage_pipeline_id_idx ON sales_pipeline_stage (pipeline_id);

ALTER TABLE sales_pipeline_stage ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_pipeline_stage FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_pipeline_stage_tenant_isolation ON sales_pipeline_stage
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_customer
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_customer (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    email TEXT,
    phone TEXT,
    website TEXT,
    billing_address TEXT,
    notes TEXT,
    owner_user_id UUID REFERENCES user_identity(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    version INT NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS sales_customer_org_id_idx ON sales_customer (org_id);
CREATE INDEX IF NOT EXISTS sales_customer_name_trgm_idx
    ON sales_customer USING gin (name gin_trgm_ops);

CREATE UNIQUE INDEX IF NOT EXISTS sales_customer_org_email_unique_idx
    ON sales_customer (org_id, lower(email))
    WHERE email IS NOT NULL AND deleted_at IS NULL;

ALTER TABLE sales_customer ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_customer FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_customer_tenant_isolation ON sales_customer
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_contact
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_contact (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    customer_id UUID NOT NULL REFERENCES sales_customer(id),
    first_name TEXT NOT NULL DEFAULT '',
    last_name TEXT NOT NULL DEFAULT '',
    email TEXT,
    phone TEXT,
    title TEXT,
    is_primary BOOLEAN NOT NULL DEFAULT false,
    owner_user_id UUID REFERENCES user_identity(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS sales_contact_org_id_idx ON sales_contact (org_id);
CREATE INDEX IF NOT EXISTS sales_contact_customer_id_idx ON sales_contact (customer_id);
CREATE INDEX IF NOT EXISTS sales_contact_name_trgm_idx
    ON sales_contact USING gin ((first_name || ' ' || last_name) gin_trgm_ops);

ALTER TABLE sales_contact ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_contact FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_contact_tenant_isolation ON sales_contact
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_lead
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_lead (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    email TEXT,
    phone TEXT,
    company_name TEXT,
    source TEXT,
    status TEXT NOT NULL DEFAULT 'new'
        CHECK (status IN ('new', 'qualified', 'disqualified', 'converted')),
    score INT NOT NULL DEFAULT 0,
    owner_user_id UUID REFERENCES user_identity(id),
    notes TEXT,
    converted_customer_id UUID REFERENCES sales_customer(id),
    converted_deal_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    version INT NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS sales_lead_org_id_idx ON sales_lead (org_id);
CREATE INDEX IF NOT EXISTS sales_lead_name_trgm_idx
    ON sales_lead USING gin (name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS sales_lead_company_name_trgm_idx
    ON sales_lead USING gin (company_name gin_trgm_ops);

ALTER TABLE sales_lead ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_lead FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_lead_tenant_isolation ON sales_lead
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_deal
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_deal (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    pipeline_id UUID NOT NULL REFERENCES sales_pipeline(id),
    stage_id UUID NOT NULL REFERENCES sales_pipeline_stage(id),
    customer_id UUID REFERENCES sales_customer(id),
    lead_id UUID REFERENCES sales_lead(id),
    name TEXT NOT NULL,
    amount_minor BIGINT NOT NULL DEFAULT 0,
    currency CHAR(3) NOT NULL DEFAULT 'USD',
    probability INT CHECK (probability IS NULL OR probability BETWEEN 0 AND 100),
    expected_close_date DATE,
    owner_user_id UUID REFERENCES user_identity(id),
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'won', 'lost')),
    won_reason TEXT,
    lost_reason TEXT,
    won_at TIMESTAMPTZ,
    lost_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    version INT NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS sales_deal_org_id_idx ON sales_deal (org_id);
CREATE INDEX IF NOT EXISTS sales_deal_org_stage_idx ON sales_deal (org_id, stage_id);
CREATE INDEX IF NOT EXISTS sales_deal_org_owner_idx ON sales_deal (org_id, owner_user_id);

ALTER TABLE sales_deal ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_deal FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_deal_tenant_isolation ON sales_deal
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Backfill lead conversion FK now that sales_deal exists.
ALTER TABLE sales_lead DROP CONSTRAINT IF EXISTS sales_lead_converted_deal_id_fkey;
ALTER TABLE sales_lead
    ADD CONSTRAINT sales_lead_converted_deal_id_fkey
    FOREIGN KEY (converted_deal_id) REFERENCES sales_deal(id);

-- ---------------------------------------------------------------------------
-- sales_deal_stage_history
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_deal_stage_history (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    deal_id UUID NOT NULL REFERENCES sales_deal(id),
    from_stage_id UUID REFERENCES sales_pipeline_stage(id),
    to_stage_id UUID NOT NULL REFERENCES sales_pipeline_stage(id),
    changed_by UUID REFERENCES user_identity(id),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    note TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS sales_deal_stage_history_org_id_idx ON sales_deal_stage_history (org_id);
CREATE INDEX IF NOT EXISTS sales_deal_stage_history_deal_id_idx ON sales_deal_stage_history (deal_id);

ALTER TABLE sales_deal_stage_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_deal_stage_history FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_deal_stage_history_tenant_isolation ON sales_deal_stage_history
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_product
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_product (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    sku TEXT,
    unit_price_minor BIGINT,
    currency CHAR(3),
    tax_group TEXT,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS sales_product_org_id_idx ON sales_product (org_id);

ALTER TABLE sales_product ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_product FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_product_tenant_isolation ON sales_product
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_quote
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_quote (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    deal_id UUID REFERENCES sales_deal(id),
    customer_id UUID NOT NULL REFERENCES sales_customer(id),
    quote_number TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'sent', 'accepted', 'rejected', 'expired')),
    version_number INT NOT NULL DEFAULT 1,
    previous_quote_id UUID REFERENCES sales_quote(id),
    currency CHAR(3) NOT NULL DEFAULT 'USD',
    subtotal_minor BIGINT NOT NULL DEFAULT 0,
    discount_minor BIGINT NOT NULL DEFAULT 0,
    tax_minor BIGINT NOT NULL DEFAULT 0,
    total_minor BIGINT NOT NULL DEFAULT 0,
    notes TEXT,
    valid_until DATE,
    accepted_at TIMESTAMPTZ,
    owner_user_id UUID REFERENCES user_identity(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    version INT NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS sales_quote_org_id_idx ON sales_quote (org_id);
CREATE INDEX IF NOT EXISTS sales_quote_customer_id_idx ON sales_quote (customer_id);
CREATE INDEX IF NOT EXISTS sales_quote_deal_id_idx ON sales_quote (deal_id);

CREATE UNIQUE INDEX IF NOT EXISTS sales_quote_org_number_version_unique_idx
    ON sales_quote (org_id, quote_number, version_number)
    WHERE deleted_at IS NULL;

ALTER TABLE sales_quote ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_quote FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_quote_tenant_isolation ON sales_quote
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_quote_line
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_quote_line (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    quote_id UUID NOT NULL REFERENCES sales_quote(id) ON DELETE CASCADE,
    position INT NOT NULL DEFAULT 0,
    product_id UUID REFERENCES sales_product(id),
    description TEXT NOT NULL DEFAULT '',
    quantity INT NOT NULL,
    unit_price_minor BIGINT NOT NULL,
    discount_minor BIGINT NOT NULL DEFAULT 0,
    tax_rate_bps INT NOT NULL DEFAULT 0,
    tax_minor BIGINT NOT NULL DEFAULT 0,
    line_total_minor BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS sales_quote_line_org_id_idx ON sales_quote_line (org_id);
CREATE INDEX IF NOT EXISTS sales_quote_line_quote_id_idx ON sales_quote_line (quote_id);

ALTER TABLE sales_quote_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_quote_line FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_quote_line_tenant_isolation ON sales_quote_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_activity
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_activity (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('note', 'call', 'meeting', 'email')),
    subject TEXT,
    body TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    customer_id UUID REFERENCES sales_customer(id),
    deal_id UUID REFERENCES sales_deal(id),
    lead_id UUID REFERENCES sales_lead(id),
    owner_user_id UUID REFERENCES user_identity(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS sales_activity_org_id_idx ON sales_activity (org_id);
CREATE INDEX IF NOT EXISTS sales_activity_customer_id_idx ON sales_activity (customer_id);
CREATE INDEX IF NOT EXISTS sales_activity_deal_id_idx ON sales_activity (deal_id);
CREATE INDEX IF NOT EXISTS sales_activity_lead_id_idx ON sales_activity (lead_id);

ALTER TABLE sales_activity ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_activity FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_activity_tenant_isolation ON sales_activity
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_idempotency
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_idempotency (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    key TEXT NOT NULL,
    scope TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, scope, key)
);

CREATE INDEX IF NOT EXISTS sales_idempotency_org_id_idx ON sales_idempotency (org_id);

ALTER TABLE sales_idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_idempotency FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_idempotency_tenant_isolation ON sales_idempotency
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_import_job
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_import_job (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'preview'
        CHECK (status IN ('preview', 'confirmed', 'failed')),
    filename TEXT NOT NULL,
    mapping JSONB NOT NULL DEFAULT '{}'::jsonb,
    preview JSONB NOT NULL DEFAULT '{}'::jsonb,
    error TEXT,
    created_by UUID REFERENCES user_identity(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS sales_import_job_org_id_idx ON sales_import_job (org_id);

ALTER TABLE sales_import_job ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_import_job FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sales_import_job_tenant_isolation ON sales_import_job
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
