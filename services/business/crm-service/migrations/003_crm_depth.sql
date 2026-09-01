-- Phase 3.5 CRM depth — orders, contracts, territories.
-- Policies use CREATE POLICY (no IF NOT EXISTS); migrate helper ignores 42710.

-- ---------------------------------------------------------------------------
-- sales_territory
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_territory (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT,
    owner_user_id UUID REFERENCES user_identity(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version INT NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS sales_territory_org_id_idx ON sales_territory (org_id);

ALTER TABLE sales_territory ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_territory FORCE ROW LEVEL SECURITY;

CREATE POLICY sales_territory_tenant_isolation ON sales_territory
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_order
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_order (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    customer_id UUID NOT NULL REFERENCES sales_customer(id),
    deal_id UUID REFERENCES sales_deal(id),
    quote_id UUID REFERENCES sales_quote(id),
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'confirmed', 'fulfilled', 'cancelled')),
    currency CHAR(3) NOT NULL DEFAULT 'USD',
    subtotal_minor BIGINT NOT NULL DEFAULT 0,
    discount_minor BIGINT NOT NULL DEFAULT 0,
    tax_minor BIGINT NOT NULL DEFAULT 0,
    total_minor BIGINT NOT NULL DEFAULT 0,
    owner_user_id UUID REFERENCES user_identity(id),
    territory_id UUID REFERENCES sales_territory(id),
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version INT NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX IF NOT EXISTS sales_order_org_quote_unique_idx
    ON sales_order (org_id, quote_id)
    WHERE quote_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS sales_order_org_id_idx ON sales_order (org_id);
CREATE INDEX IF NOT EXISTS sales_order_org_status_idx ON sales_order (org_id, status);

ALTER TABLE sales_order ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_order FORCE ROW LEVEL SECURITY;

CREATE POLICY sales_order_tenant_isolation ON sales_order
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_order_line
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_order_line (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    order_id UUID NOT NULL REFERENCES sales_order(id) ON DELETE CASCADE,
    position INT NOT NULL DEFAULT 0,
    product_id UUID REFERENCES sales_product(id),
    description TEXT NOT NULL DEFAULT '',
    quantity INT NOT NULL,
    unit_price_minor BIGINT NOT NULL,
    discount_minor BIGINT NOT NULL DEFAULT 0,
    tax_rate_bps INT NOT NULL DEFAULT 0,
    tax_minor BIGINT NOT NULL DEFAULT 0,
    line_total_minor BIGINT NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS sales_order_line_org_id_idx ON sales_order_line (org_id);
CREATE INDEX IF NOT EXISTS sales_order_line_order_id_idx ON sales_order_line (order_id);

ALTER TABLE sales_order_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_order_line FORCE ROW LEVEL SECURITY;

CREATE POLICY sales_order_line_tenant_isolation ON sales_order_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_contract
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_contract (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    customer_id UUID NOT NULL REFERENCES sales_customer(id),
    deal_id UUID REFERENCES sales_deal(id),
    order_id UUID REFERENCES sales_order(id),
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'active', 'expired', 'cancelled')),
    term_months INT,
    start_date DATE,
    end_date DATE,
    value_minor BIGINT NOT NULL DEFAULT 0,
    currency CHAR(3) NOT NULL DEFAULT 'USD',
    auto_renew BOOLEAN NOT NULL DEFAULT false,
    renewal_notice_days INT NOT NULL DEFAULT 30,
    owner_user_id UUID REFERENCES user_identity(id),
    published_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version INT NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS sales_contract_org_id_idx ON sales_contract (org_id);
CREATE INDEX IF NOT EXISTS sales_contract_end_date_idx ON sales_contract (end_date);

ALTER TABLE sales_contract ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_contract FORCE ROW LEVEL SECURITY;

CREATE POLICY sales_contract_tenant_isolation ON sales_contract
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_territory_assignment
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales_territory_assignment (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    territory_id UUID NOT NULL REFERENCES sales_territory(id) ON DELETE CASCADE,
    customer_id UUID REFERENCES sales_customer(id),
    deal_id UUID REFERENCES sales_deal(id),
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((customer_id IS NOT NULL) <> (deal_id IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS sales_territory_assignment_org_id_idx
    ON sales_territory_assignment (org_id);
CREATE INDEX IF NOT EXISTS sales_territory_assignment_territory_id_idx
    ON sales_territory_assignment (territory_id);

ALTER TABLE sales_territory_assignment ENABLE ROW LEVEL SECURITY;
ALTER TABLE sales_territory_assignment FORCE ROW LEVEL SECURITY;

CREATE POLICY sales_territory_assignment_tenant_isolation ON sales_territory_assignment
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- sales_customer — territory + opaque finance dunning profile public id
-- ---------------------------------------------------------------------------
ALTER TABLE sales_customer
    ADD COLUMN IF NOT EXISTS territory_id UUID REFERENCES sales_territory(id);
ALTER TABLE sales_customer
    ADD COLUMN IF NOT EXISTS dunning_profile_id TEXT;
