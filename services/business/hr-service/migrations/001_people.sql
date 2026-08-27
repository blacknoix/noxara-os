-- Phase 2.1 People (HR) schema.
-- Tenant isolation via org_id + FORCE RLS.
-- department_id / user_id are opaque UUIDs (Workspace/identity identifiers —
-- never join workspace department tables or invent a second department SoT).
-- Soft delete via deleted_at. Optimistic concurrency via version.
-- Restricted fields stored as encrypted BYTEA (app-level envelope AES-256-GCM).

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- ---------------------------------------------------------------------------
-- Employees (master record)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_employee (
    id                      UUID PRIMARY KEY,
    org_id                  UUID NOT NULL REFERENCES organization(id),
    public_id               TEXT NOT NULL,
    -- Opaque link to user_identity (usr_…); not a second user store.
    user_id                 UUID,
    display_name            TEXT NOT NULL,
    legal_first_name        TEXT,
    legal_last_name         TEXT,
    work_email              TEXT,
    personal_email          TEXT,
    phone                   TEXT,
    title                   TEXT,
    status                  TEXT NOT NULL DEFAULT 'active' CHECK (status IN (
        'draft', 'onboarding', 'active', 'on_leave', 'offboarding', 'terminated'
    )),
    start_date              DATE,
    end_date                DATE,
    location                TEXT,
    -- Opaque Workspace department UUID (dep_ public id stored alongside).
    department_id           UUID,
    department_public_id    TEXT,
    -- Reporting line mastered in People (self-FK).
    manager_employee_id     UUID REFERENCES people_employee(id),
    -- Scope owner for list SQL (defaults to linked user_id or creator).
    owner_user_id           UUID NOT NULL,
    -- Encrypted restricted payloads (NULL when unset). Never log plaintext.
    government_id_ciphertext BYTEA,
    bank_details_ciphertext  BYTEA,
    tax_id_ciphertext        BYTEA,
    encryption_key_id       TEXT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at              TIMESTAMPTZ,
    version                 INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_employee_org_idx
    ON people_employee (org_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_employee_org_user_idx
    ON people_employee (org_id, user_id) WHERE deleted_at IS NULL AND user_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS people_employee_org_owner_idx
    ON people_employee (org_id, owner_user_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_employee_org_status_idx
    ON people_employee (org_id, status) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_employee_org_dept_idx
    ON people_employee (org_id, department_id)
    WHERE deleted_at IS NULL AND department_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS people_employee_org_name_trgm_idx
    ON people_employee USING gin (display_name gin_trgm_ops)
    WHERE deleted_at IS NULL;
ALTER TABLE people_employee ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_employee FORCE ROW LEVEL SECURITY;
CREATE POLICY people_employee_tenant_isolation ON people_employee
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Employment contracts (effective-dated)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_employment_contract (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    employee_id     UUID NOT NULL REFERENCES people_employee(id),
    contract_type   TEXT NOT NULL DEFAULT 'full_time',
    title           TEXT,
    effective_from  DATE NOT NULL,
    effective_to    DATE,
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_contract_org_emp_idx
    ON people_employment_contract (org_id, employee_id) WHERE deleted_at IS NULL;
ALTER TABLE people_employment_contract ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_employment_contract FORCE ROW LEVEL SECURITY;
CREATE POLICY people_contract_tenant_isolation ON people_employment_contract
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Compensation components (versioned / effective-dated; amount encrypted)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_compensation_component (
    id                      UUID PRIMARY KEY,
    org_id                  UUID NOT NULL REFERENCES organization(id),
    public_id               TEXT NOT NULL,
    employee_id             UUID NOT NULL REFERENCES people_employee(id),
    contract_id             UUID REFERENCES people_employment_contract(id),
    component_type          TEXT NOT NULL DEFAULT 'base_salary',
    label                   TEXT NOT NULL,
    -- Money: integer minor units + ISO 4217 (plaintext currency; amount encrypted).
    amount_minor_ciphertext BYTEA NOT NULL,
    currency                CHAR(3) NOT NULL,
    encryption_key_id       TEXT NOT NULL,
    effective_from          DATE NOT NULL,
    effective_to            DATE,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at              TIMESTAMPTZ,
    version                 INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_comp_org_emp_idx
    ON people_compensation_component (org_id, employee_id) WHERE deleted_at IS NULL;
ALTER TABLE people_compensation_component ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_compensation_component FORCE ROW LEVEL SECURITY;
CREATE POLICY people_comp_tenant_isolation ON people_compensation_component
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Documents (file-service fil_ id opaque; expiry tracking)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_document (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    employee_id     UUID NOT NULL REFERENCES people_employee(id),
    title           TEXT NOT NULL,
    doc_type        TEXT NOT NULL DEFAULT 'other',
    file_id         TEXT,
    expires_at      DATE,
    collected       BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_document_org_emp_idx
    ON people_document (org_id, employee_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_document_org_expiry_idx
    ON people_document (org_id, expires_at)
    WHERE deleted_at IS NULL AND expires_at IS NOT NULL;
ALTER TABLE people_document ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_document FORCE ROW LEVEL SECURITY;
CREATE POLICY people_document_tenant_isolation ON people_document
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- HR assets (simple assignment list — not inventory-service)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_asset (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    employee_id     UUID NOT NULL REFERENCES people_employee(id),
    label           TEXT NOT NULL,
    asset_tag       TEXT,
    status          TEXT NOT NULL DEFAULT 'assigned' CHECK (status IN (
        'assigned', 'returned', 'lost', 'pending_return'
    )),
    assigned_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    returned_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_asset_org_emp_idx
    ON people_asset (org_id, employee_id) WHERE deleted_at IS NULL;
ALTER TABLE people_asset ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_asset FORCE ROW LEVEL SECURITY;
CREATE POLICY people_asset_tenant_isolation ON people_asset
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Onboarding / offboarding tasks
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_task (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    employee_id     UUID NOT NULL REFERENCES people_employee(id),
    kind            TEXT NOT NULL CHECK (kind IN ('onboarding', 'offboarding', 'document', 'asset', 'other')),
    title           TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending', 'in_progress', 'done', 'cancelled', 'compensated'
    )),
    assignee_user_id UUID,
    due_at          TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    workflow_id     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_task_org_emp_idx
    ON people_task (org_id, employee_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_task_org_wf_idx
    ON people_task (org_id, workflow_id) WHERE deleted_at IS NULL AND workflow_id IS NOT NULL;
ALTER TABLE people_task ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_task FORCE ROW LEVEL SECURITY;
CREATE POLICY people_task_tenant_isolation ON people_task
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Timeline / activity feed (HR-owned projection)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_timeline_event (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    employee_id     UUID NOT NULL REFERENCES people_employee(id),
    event_type      TEXT NOT NULL,
    summary         TEXT NOT NULL,
    metadata        JSONB NOT NULL DEFAULT '{}'::jsonb,
    occurred_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor_user_id   UUID
);
CREATE INDEX IF NOT EXISTS people_timeline_org_emp_idx
    ON people_timeline_event (org_id, employee_id, occurred_at DESC);
ALTER TABLE people_timeline_event ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_timeline_event FORCE ROW LEVEL SECURITY;
CREATE POLICY people_timeline_tenant_isolation ON people_timeline_event
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Workflow run bookkeeping (idempotent starts)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_workflow_run (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    employee_id     UUID NOT NULL REFERENCES people_employee(id),
    workflow_type   TEXT NOT NULL,
    workflow_id     TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'running',
    compensation_of UUID,
    error_detail    TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, workflow_id)
);
CREATE INDEX IF NOT EXISTS people_workflow_org_emp_idx
    ON people_workflow_run (org_id, employee_id);
ALTER TABLE people_workflow_run ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_workflow_run FORCE ROW LEVEL SECURITY;
CREATE POLICY people_workflow_tenant_isolation ON people_workflow_run
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Idempotency
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_idempotency (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    scope           TEXT NOT NULL,
    key             TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body   JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, scope, key)
);
ALTER TABLE people_idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_idempotency FORCE ROW LEVEL SECURITY;
CREATE POLICY people_idempotency_tenant_isolation ON people_idempotency
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
