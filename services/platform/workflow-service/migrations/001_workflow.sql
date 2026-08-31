-- Phase 3.1 — Configurable workflow engine (org-scoped definitions + instances).
-- IDs: wfd_ (definition), wfv_ (version), wfi_ (instance). RLS on every table.

CREATE TABLE IF NOT EXISTS workflow_definition (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'published', 'archived')),
    -- Creator whose permissions bound every action in this definition.
    created_by UUID NOT NULL,
    updated_by UUID NOT NULL,
    current_published_version INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);

CREATE INDEX IF NOT EXISTS workflow_definition_org_status_idx
    ON workflow_definition (org_id, status);

ALTER TABLE workflow_definition ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_definition FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS workflow_definition_tenant_isolation ON workflow_definition;
CREATE POLICY workflow_definition_tenant_isolation ON workflow_definition
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Immutable published (and draft snapshot) versions. In-flight instances pin a version.
CREATE TABLE IF NOT EXISTS workflow_definition_version (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL,
    definition_id UUID NOT NULL REFERENCES workflow_definition(id),
    version INT NOT NULL,
    -- Full graph: trigger, nodes (actions/conditions/timers/branches/human), entry.
    graph JSONB NOT NULL,
    -- Permissions required by actions — validated at save + publish + run.
    required_permissions TEXT[] NOT NULL DEFAULT '{}',
    published_at TIMESTAMPTZ,
    published_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, definition_id, version)
);

CREATE INDEX IF NOT EXISTS workflow_definition_version_def_idx
    ON workflow_definition_version (org_id, definition_id, version DESC);

ALTER TABLE workflow_definition_version ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_definition_version FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS workflow_definition_version_tenant_isolation ON workflow_definition_version;
CREATE POLICY workflow_definition_version_tenant_isolation ON workflow_definition_version
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS workflow_instance (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL,
    definition_id UUID NOT NULL REFERENCES workflow_definition(id),
    -- Pinned version — publishing a new definition does NOT mutate this.
    version_id UUID NOT NULL REFERENCES workflow_definition_version(id),
    version_number INT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN (
            'running', 'waiting', 'completed', 'failed', 'cancelled', 'sla_breached'
        )),
    -- Actor recorded at start; activities run on_behalf_of this user (never superuser).
    actor_user_id UUID NOT NULL,
    created_by UUID NOT NULL,
    trigger_event JSONB,
    trigger_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    current_node_id TEXT,
    step_count INT NOT NULL DEFAULT 0,
    error_message TEXT,
    waiting_until TIMESTAMPTZ,
    sla_deadline TIMESTAMPTZ,
    temporal_workflow_id TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, temporal_workflow_id)
);

CREATE INDEX IF NOT EXISTS workflow_instance_org_status_idx
    ON workflow_instance (org_id, status);
CREATE INDEX IF NOT EXISTS workflow_instance_org_def_idx
    ON workflow_instance (org_id, definition_id);
CREATE INDEX IF NOT EXISTS workflow_instance_sla_idx
    ON workflow_instance (org_id, sla_deadline)
    WHERE status IN ('running', 'waiting') AND sla_deadline IS NOT NULL;

ALTER TABLE workflow_instance ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_instance FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS workflow_instance_tenant_isolation ON workflow_instance;
CREATE POLICY workflow_instance_tenant_isolation ON workflow_instance
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS workflow_instance_step (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    instance_id UUID NOT NULL REFERENCES workflow_instance(id),
    step_index INT NOT NULL,
    node_id TEXT NOT NULL,
    node_type TEXT NOT NULL,
    action_key TEXT,
    status TEXT NOT NULL
        CHECK (status IN ('planned', 'ok', 'skipped', 'failed', 'waiting', 'denied')),
    detail JSONB NOT NULL DEFAULT '{}'::jsonb,
    permission_checked TEXT,
    permission_allowed BOOLEAN,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, instance_id, step_index)
);

CREATE INDEX IF NOT EXISTS workflow_instance_step_inst_idx
    ON workflow_instance_step (org_id, instance_id, step_index);

ALTER TABLE workflow_instance_step ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_instance_step FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS workflow_instance_step_tenant_isolation ON workflow_instance_step;
CREATE POLICY workflow_instance_step_tenant_isolation ON workflow_instance_step
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Per-org runaway bounds (concurrency + per-instance step cap).
CREATE TABLE IF NOT EXISTS workflow_org_bounds (
    org_id UUID PRIMARY KEY REFERENCES organization(id),
    max_concurrent INT NOT NULL DEFAULT 50 CHECK (max_concurrent > 0 AND max_concurrent <= 1000),
    max_steps_per_instance INT NOT NULL DEFAULT 100
        CHECK (max_steps_per_instance > 0 AND max_steps_per_instance <= 10000),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID
);

ALTER TABLE workflow_org_bounds ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_org_bounds FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS workflow_org_bounds_tenant_isolation ON workflow_org_bounds;
CREATE POLICY workflow_org_bounds_tenant_isolation ON workflow_org_bounds
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS workflow_idempotency (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, scope, key)
);

ALTER TABLE workflow_idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_idempotency FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS workflow_idempotency_tenant_isolation ON workflow_idempotency;
CREATE POLICY workflow_idempotency_tenant_isolation ON workflow_idempotency
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Seed fixture templates are inserted per-org on first publish/list via application code
-- (not global rows — keep RLS clean).
