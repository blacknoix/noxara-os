-- Phase 2.5 Inventory & Procurement schema.
-- Tenant isolation via org_id + FORCE RLS (mirrors people_employee pattern).
-- Soft delete via deleted_at where the entity is a master record.
-- Optimistic concurrency via version.
--
-- Valuation: Weighted Average (see docs/adrs/023-inventory-valuation-wavg.md).
-- inventory_stock_level is a CACHE only — qty_on_hand / avg_unit_cost_minor
-- are derived from inventory_stock_movement (append-only). The cache is
-- reconciled against the movement ledger; drift raises an alert row + an
-- outbox event, it is NEVER silently rewritten by app code.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ---------------------------------------------------------------------------
-- Warehouses
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_warehouse (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    code            TEXT NOT NULL,
    name            TEXT NOT NULL,
    location        TEXT,
    is_active       BOOLEAN NOT NULL DEFAULT true,
    owner_user_id   UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS inventory_warehouse_org_code_idx
    ON inventory_warehouse (org_id, code) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS inventory_warehouse_org_idx
    ON inventory_warehouse (org_id) WHERE deleted_at IS NULL;
ALTER TABLE inventory_warehouse ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_warehouse FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_warehouse_tenant_isolation ON inventory_warehouse
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Items (SKU master)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_item (
    id                      UUID PRIMARY KEY,
    org_id                  UUID NOT NULL REFERENCES organization(id),
    public_id               TEXT NOT NULL,
    sku                     TEXT NOT NULL,
    name                    TEXT NOT NULL,
    description             TEXT,
    uom                     TEXT NOT NULL DEFAULT 'each',
    currency                CHAR(3) NOT NULL,
    reorder_point_qty       BIGINT NOT NULL DEFAULT 0,
    allow_negative_stock    BOOLEAN NOT NULL DEFAULT false,
    is_active               BOOLEAN NOT NULL DEFAULT true,
    owner_user_id           UUID NOT NULL,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at              TIMESTAMPTZ,
    version                 INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS inventory_item_org_sku_idx
    ON inventory_item (org_id, sku) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS inventory_item_org_idx
    ON inventory_item (org_id) WHERE deleted_at IS NULL;
ALTER TABLE inventory_item ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_item FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_item_tenant_isolation ON inventory_item
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Stock level — CACHE only, never the source of truth. See stock.rs.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_stock_level (
    org_id              UUID NOT NULL REFERENCES organization(id),
    warehouse_id        UUID NOT NULL REFERENCES inventory_warehouse(id),
    item_id              UUID NOT NULL REFERENCES inventory_item(id),
    qty_on_hand          BIGINT NOT NULL DEFAULT 0,
    avg_unit_cost_minor  BIGINT NOT NULL DEFAULT 0,
    last_movement_at     TIMESTAMPTZ,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, warehouse_id, item_id)
);
ALTER TABLE inventory_stock_level ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_stock_level FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_stock_level_tenant_isolation ON inventory_stock_level
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Stock movements — APPEND-ONLY ledger (source of truth for qty on hand).
-- App code never UPDATEs or DELETEs a posted movement; corrections are new
-- 'adjustment' movements. A trigger rejects UPDATE/DELETE defensively.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_stock_movement (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    warehouse_id    UUID NOT NULL REFERENCES inventory_warehouse(id),
    item_id         UUID NOT NULL REFERENCES inventory_item(id),
    qty_delta       BIGINT NOT NULL,
    unit_cost_minor BIGINT NOT NULL DEFAULT 0,
    currency        CHAR(3) NOT NULL,
    movement_type   TEXT NOT NULL CHECK (movement_type IN (
        'receipt', 'issue', 'adjustment', 'transfer_in', 'transfer_out', 'return'
    )),
    source_type     TEXT,
    source_id       UUID,
    idempotency_key TEXT,
    memo            TEXT,
    created_by      UUID,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS inventory_stock_movement_org_idem_idx
    ON inventory_stock_movement (org_id, idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS inventory_stock_movement_org_wh_item_idx
    ON inventory_stock_movement (org_id, warehouse_id, item_id, created_at);
CREATE INDEX IF NOT EXISTS inventory_stock_movement_org_source_idx
    ON inventory_stock_movement (org_id, source_type, source_id) WHERE source_id IS NOT NULL;
ALTER TABLE inventory_stock_movement ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_stock_movement FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_stock_movement_tenant_isolation ON inventory_stock_movement
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE OR REPLACE FUNCTION inventory_stock_movement_append_only()
RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'inventory_stock_movement is append-only; % not permitted', TG_OP;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS inventory_stock_movement_no_update ON inventory_stock_movement;
CREATE TRIGGER inventory_stock_movement_no_update
    BEFORE UPDATE OR DELETE ON inventory_stock_movement
    FOR EACH ROW EXECUTE FUNCTION inventory_stock_movement_append_only();

-- ---------------------------------------------------------------------------
-- Suppliers
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_supplier (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    email           TEXT,
    phone           TEXT,
    currency        CHAR(3) NOT NULL,
    payment_terms   TEXT,
    owner_user_id   UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_supplier_org_idx
    ON inventory_supplier (org_id) WHERE deleted_at IS NULL;
ALTER TABLE inventory_supplier ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_supplier FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_supplier_tenant_isolation ON inventory_supplier
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Purchase requests (thin budget check only — full budgeting is a later phase)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_purchase_request (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'pending_approval', 'approved', 'rejected', 'cancelled', 'converted'
    )),
    requester_user_id   UUID NOT NULL,
    approval_id         TEXT,
    currency            CHAR(3) NOT NULL,
    total_amount_minor  BIGINT NOT NULL DEFAULT 0,
    budget_account_code TEXT,
    notes               TEXT,
    owner_user_id       UUID NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_pr_org_status_idx
    ON inventory_purchase_request (org_id, status);
ALTER TABLE inventory_purchase_request ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_purchase_request FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_purchase_request_tenant_isolation ON inventory_purchase_request
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS inventory_purchase_request_line (
    id                          UUID PRIMARY KEY,
    org_id                      UUID NOT NULL REFERENCES organization(id),
    public_id                   TEXT NOT NULL,
    request_id                  UUID NOT NULL REFERENCES inventory_purchase_request(id),
    item_id                     UUID NOT NULL REFERENCES inventory_item(id),
    qty                         BIGINT NOT NULL CHECK (qty > 0),
    unit_cost_estimate_minor    BIGINT NOT NULL CHECK (unit_cost_estimate_minor >= 0),
    line_amount_minor           BIGINT NOT NULL CHECK (line_amount_minor >= 0),
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_prl_org_request_idx
    ON inventory_purchase_request_line (org_id, request_id);
ALTER TABLE inventory_purchase_request_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_purchase_request_line FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_purchase_request_line_tenant_isolation ON inventory_purchase_request_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Purchase orders
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_purchase_order (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    supplier_id         UUID NOT NULL REFERENCES inventory_supplier(id),
    purchase_request_id UUID REFERENCES inventory_purchase_request(id),
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'issued', 'partially_received', 'received', 'cancelled'
    )),
    currency            CHAR(3) NOT NULL,
    total_amount_minor  BIGINT NOT NULL DEFAULT 0,
    issued_at           TIMESTAMPTZ,
    owner_user_id       UUID NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_po_org_status_idx
    ON inventory_purchase_order (org_id, status);
CREATE INDEX IF NOT EXISTS inventory_po_org_supplier_idx
    ON inventory_purchase_order (org_id, supplier_id);
ALTER TABLE inventory_purchase_order ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_purchase_order FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_purchase_order_tenant_isolation ON inventory_purchase_order
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS inventory_purchase_order_line (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    order_id            UUID NOT NULL REFERENCES inventory_purchase_order(id),
    item_id             UUID NOT NULL REFERENCES inventory_item(id),
    warehouse_id        UUID NOT NULL REFERENCES inventory_warehouse(id),
    qty_ordered         BIGINT NOT NULL CHECK (qty_ordered > 0),
    qty_received        BIGINT NOT NULL DEFAULT 0 CHECK (qty_received >= 0),
    unit_cost_minor     BIGINT NOT NULL CHECK (unit_cost_minor >= 0),
    line_amount_minor   BIGINT NOT NULL CHECK (line_amount_minor >= 0),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_pol_org_order_idx
    ON inventory_purchase_order_line (org_id, order_id);
ALTER TABLE inventory_purchase_order_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_purchase_order_line FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_purchase_order_line_tenant_isolation ON inventory_purchase_order_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Goods receipts (GRN) — partial receipt supported.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_goods_receipt (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    purchase_order_id   UUID NOT NULL REFERENCES inventory_purchase_order(id),
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'posted')),
    received_at         TIMESTAMPTZ,
    journal_public_id   TEXT,
    owner_user_id       UUID NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_grn_org_po_idx
    ON inventory_goods_receipt (org_id, purchase_order_id);
ALTER TABLE inventory_goods_receipt ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_goods_receipt FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_goods_receipt_tenant_isolation ON inventory_goods_receipt
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS inventory_goods_receipt_line (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    receipt_id      UUID NOT NULL REFERENCES inventory_goods_receipt(id),
    po_line_id      UUID NOT NULL REFERENCES inventory_purchase_order_line(id),
    item_id         UUID NOT NULL REFERENCES inventory_item(id),
    warehouse_id    UUID NOT NULL REFERENCES inventory_warehouse(id),
    qty_received    BIGINT NOT NULL CHECK (qty_received > 0),
    unit_cost_minor BIGINT NOT NULL CHECK (unit_cost_minor >= 0),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_grl_org_receipt_idx
    ON inventory_goods_receipt_line (org_id, receipt_id);
ALTER TABLE inventory_goods_receipt_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_goods_receipt_line FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_goods_receipt_line_tenant_isolation ON inventory_goods_receipt_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Fixed assets (inventory-owned; NOT people_asset / HR assignment list)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_asset (
    id                              UUID PRIMARY KEY,
    org_id                          UUID NOT NULL REFERENCES organization(id),
    public_id                       TEXT NOT NULL,
    item_id                         UUID REFERENCES inventory_item(id),
    name                            TEXT NOT NULL,
    asset_tag                       TEXT,
    status                          TEXT NOT NULL DEFAULT 'in_stock' CHECK (status IN (
        'in_stock', 'assigned', 'maintenance', 'disposed'
    )),
    acquisition_cost_minor          BIGINT NOT NULL DEFAULT 0,
    currency                        CHAR(3) NOT NULL,
    acquired_at                     DATE,
    useful_life_months              INT NOT NULL DEFAULT 36,
    salvage_minor                   BIGINT NOT NULL DEFAULT 0,
    accumulated_depreciation_minor  BIGINT NOT NULL DEFAULT 0,
    last_depreciated_at             DATE,
    owner_user_id                   UUID NOT NULL,
    created_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at                      TIMESTAMPTZ,
    version                         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_asset_org_idx
    ON inventory_asset (org_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS inventory_asset_org_status_idx
    ON inventory_asset (org_id, status) WHERE deleted_at IS NULL;
ALTER TABLE inventory_asset ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_asset FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_asset_tenant_isolation ON inventory_asset
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS inventory_asset_assignment (
    id                          UUID PRIMARY KEY,
    org_id                      UUID NOT NULL REFERENCES organization(id),
    public_id                   TEXT NOT NULL,
    asset_id                    UUID NOT NULL REFERENCES inventory_asset(id),
    -- Opaque HR employee public id (emp_…) — no FK to people tables.
    assignee_employee_public_id TEXT NOT NULL,
    assigned_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
    returned_at                 TIMESTAMPTZ,
    notes                       TEXT,
    created_by                  UUID,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_asset_assignment_org_asset_idx
    ON inventory_asset_assignment (org_id, asset_id);
ALTER TABLE inventory_asset_assignment ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_asset_assignment FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_asset_assignment_tenant_isolation ON inventory_asset_assignment
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS inventory_maintenance_schedule (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    asset_id            UUID NOT NULL REFERENCES inventory_asset(id),
    title               TEXT NOT NULL,
    interval_days       INT NOT NULL CHECK (interval_days > 0),
    next_due_at         DATE NOT NULL,
    last_completed_at   DATE,
    notes               TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS inventory_maint_org_due_idx
    ON inventory_maintenance_schedule (org_id, next_due_at);
CREATE INDEX IF NOT EXISTS inventory_maint_org_asset_idx
    ON inventory_maintenance_schedule (org_id, asset_id);
ALTER TABLE inventory_maintenance_schedule ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_maintenance_schedule FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_maintenance_schedule_tenant_isolation ON inventory_maintenance_schedule
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Idempotency
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_idempotency (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    scope           TEXT NOT NULL,
    key             TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body   JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, scope, key)
);
ALTER TABLE inventory_idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_idempotency FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_idempotency_tenant_isolation ON inventory_idempotency
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Stock drift alerts — reconcile finds cache vs. ledger mismatch; the cache
-- is NEVER silently rewritten, only alerted (see stock::reconcile_stock).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_drift_alert (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    warehouse_id        UUID NOT NULL REFERENCES inventory_warehouse(id),
    item_id             UUID NOT NULL REFERENCES inventory_item(id),
    cached_qty          BIGINT NOT NULL,
    movement_sum_qty    BIGINT NOT NULL,
    detected_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    alerted             BOOLEAN NOT NULL DEFAULT true
);
CREATE INDEX IF NOT EXISTS inventory_drift_alert_org_idx
    ON inventory_drift_alert (org_id, detected_at DESC);
ALTER TABLE inventory_drift_alert ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_drift_alert FORCE ROW LEVEL SECURITY;
CREATE POLICY inventory_drift_alert_tenant_isolation ON inventory_drift_alert
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
