-- Phase 1.2 Workspace: orgs settings, roles, permissions, teams, departments,
-- invitations, provisioning commands. Extends Phase 1.1 membership model.

-- ---------------------------------------------------------------------------
-- Permission catalogue (synced from crates/authz; CI fails on divergence)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS permission_definition (
    id TEXT PRIMARY KEY,
    context TEXT NOT NULL,
    resource TEXT NOT NULL,
    action TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    sensitive BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (id = context || '.' || resource || '.' || action)
);

-- ---------------------------------------------------------------------------
-- Organization settings / plan / branding
-- ---------------------------------------------------------------------------
ALTER TABLE organization
    ADD COLUMN IF NOT EXISTS currency TEXT NOT NULL DEFAULT 'USD',
    ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'UTC',
    ADD COLUMN IF NOT EXISTS fiscal_year_start_month INT NOT NULL DEFAULT 1
        CHECK (fiscal_year_start_month BETWEEN 1 AND 12),
    ADD COLUMN IF NOT EXISTS numbering_series JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS branding JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS business_type TEXT NOT NULL DEFAULT 'general',
    ADD COLUMN IF NOT EXISTS plan TEXT NOT NULL DEFAULT 'starter',
    ADD COLUMN IF NOT EXISTS feature_flags JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS seed_defaults JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ---------------------------------------------------------------------------
-- Org roles (system templates copied per-org + custom roles)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS org_role (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    system_key TEXT,
    is_system BOOLEAN NOT NULL DEFAULT false,
    -- Approval limits (Phase 1.7 engine consumes these later)
    approval_limit_amount_minor BIGINT,
    approval_limit_currency TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name),
    UNIQUE (org_id, system_key)
);

CREATE INDEX IF NOT EXISTS org_role_org_id_idx ON org_role (org_id);

ALTER TABLE org_role ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_role FORCE ROW LEVEL SECURITY;

CREATE POLICY org_role_tenant_isolation ON org_role
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS role_permission (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    role_id UUID NOT NULL REFERENCES org_role(id) ON DELETE CASCADE,
    permission_id TEXT NOT NULL REFERENCES permission_definition(id),
    effect TEXT NOT NULL CHECK (effect IN ('allow', 'deny')),
    scope TEXT NOT NULL DEFAULT 'organization'
        CHECK (scope IN ('own', 'team', 'department', 'organization')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (role_id, permission_id, effect)
);

CREATE INDEX IF NOT EXISTS role_permission_org_id_idx ON role_permission (org_id);
CREATE INDEX IF NOT EXISTS role_permission_role_id_idx ON role_permission (role_id);

ALTER TABLE role_permission ENABLE ROW LEVEL SECURITY;
ALTER TABLE role_permission FORCE ROW LEVEL SECURITY;

CREATE POLICY role_permission_tenant_isolation ON role_permission
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Memberships: status + role_id (keep denormalized role for JWT/MFA)
-- ---------------------------------------------------------------------------
ALTER TABLE membership
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active',
    ADD COLUMN IF NOT EXISTS role_id UUID REFERENCES org_role(id),
    ADD COLUMN IF NOT EXISTS team_id UUID,
    ADD COLUMN IF NOT EXISTS department_id UUID,
    ADD COLUMN IF NOT EXISTS suspended_at TIMESTAMPTZ;

-- Widen role CHECK for system templates.
ALTER TABLE membership DROP CONSTRAINT IF EXISTS membership_role_check;
ALTER TABLE membership
    ADD CONSTRAINT membership_role_check
    CHECK (role IN ('owner', 'admin', 'finance', 'sales', 'manager', 'member', 'read_only'));

ALTER TABLE membership DROP CONSTRAINT IF EXISTS membership_status_check;
ALTER TABLE membership
    ADD CONSTRAINT membership_status_check
    CHECK (status IN ('active', 'suspended', 'revoked'));

-- Backfill status from revoked_at
UPDATE membership SET status = 'revoked' WHERE revoked_at IS NOT NULL AND status = 'active';

-- ---------------------------------------------------------------------------
-- Teams & departments (light hierarchy)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS department (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    parent_id UUID REFERENCES department(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

CREATE INDEX IF NOT EXISTS department_org_id_idx ON department (org_id);

ALTER TABLE department ENABLE ROW LEVEL SECURITY;
ALTER TABLE department FORCE ROW LEVEL SECURITY;

CREATE POLICY department_tenant_isolation ON department
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS team (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    department_id UUID REFERENCES department(id),
    parent_team_id UUID REFERENCES team(id),
    lead_user_id UUID REFERENCES user_identity(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

CREATE INDEX IF NOT EXISTS team_org_id_idx ON team (org_id);

ALTER TABLE team ENABLE ROW LEVEL SECURITY;
ALTER TABLE team FORCE ROW LEVEL SECURITY;

CREATE POLICY team_tenant_isolation ON team
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- FKs from membership now that team/department exist (idempotent recreate).
ALTER TABLE membership DROP CONSTRAINT IF EXISTS membership_team_id_fkey;
ALTER TABLE membership
    ADD CONSTRAINT membership_team_id_fkey
    FOREIGN KEY (team_id) REFERENCES team(id);

ALTER TABLE membership DROP CONSTRAINT IF EXISTS membership_department_id_fkey;
ALTER TABLE membership
    ADD CONSTRAINT membership_department_id_fkey
    FOREIGN KEY (department_id) REFERENCES department(id);

-- ---------------------------------------------------------------------------
-- Invitations
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS invitation (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    email_normalized TEXT NOT NULL,
    role_id UUID NOT NULL REFERENCES org_role(id),
    invited_by UUID NOT NULL REFERENCES user_identity(id),
    token_hash TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'accepted', 'revoked', 'expired')),
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    accepted_user_id UUID REFERENCES user_identity(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS invitation_org_id_idx ON invitation (org_id);
CREATE INDEX IF NOT EXISTS invitation_email_idx ON invitation (email_normalized);

ALTER TABLE invitation ENABLE ROW LEVEL SECURITY;
ALTER TABLE invitation FORCE ROW LEVEL SECURITY;

CREATE POLICY invitation_tenant_isolation ON invitation
    USING (
        org_id = NULLIF(current_setting('app.org_id', true), '')::uuid
        OR token_hash = NULLIF(current_setting('app.invite_token_hash', true), '')
    )
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Durable OrgProvisioning command (Temporal follow-up documented in ADR 017)
-- Idempotent ID: {org_public_id}:OrgProvisioning:{org_public_id}
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS workspace_command (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    command_type TEXT NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    attempts INT NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS workspace_command_pending_idx
    ON workspace_command (created_at)
    WHERE status IN ('pending', 'failed');

ALTER TABLE workspace_command ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_command FORCE ROW LEVEL SECURITY;

CREATE POLICY workspace_command_tenant_isolation ON workspace_command
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Stub seed tables (NOT CRM) — provisioning defaults by business_type
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS workspace_seed_pipeline_stage (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    name TEXT NOT NULL,
    position INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

ALTER TABLE workspace_seed_pipeline_stage ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_seed_pipeline_stage FORCE ROW LEVEL SECURITY;

CREATE POLICY workspace_seed_pipeline_stage_tenant ON workspace_seed_pipeline_stage
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS workspace_seed_expense_category (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

ALTER TABLE workspace_seed_expense_category ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_seed_expense_category FORCE ROW LEVEL SECURITY;

CREATE POLICY workspace_seed_expense_category_tenant ON workspace_seed_expense_category
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Last-Owner invariant is enforced in application code (last_owner.rs).
-- Optional SQL helper omitted to keep migration splitter simple (no $$ bodies).
-- ---------------------------------------------------------------------------
