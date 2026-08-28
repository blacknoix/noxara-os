-- Phase 1.7 Approval engine (Operations).
-- Tenant isolation via org_id + FORCE RLS.
-- Policy versions are immutable snapshots; in-flight approvals keep the version
-- that routed them. Decisions are append-only (single terminal status).

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ---------------------------------------------------------------------------
-- Versioned approval policies
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_approval_policy (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    subject_type    TEXT NOT NULL CHECK (subject_type IN (
        'expense', 'quote_discount', 'generic',
        'leave_request', 'payroll_run', 'purchase_request'
    )),
    -- When false, policy is not considered for new routing.
    is_active       BOOLEAN NOT NULL DEFAULT true,
    -- Monotonic version counter; each publish inserts an immutable row below.
    current_version INT NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS operations_approval_policy_org_idx
    ON operations_approval_policy (org_id, subject_type)
    WHERE is_active;
-- Widen subject_type for pre-existing databases created before HR/Payroll
-- (leave_request, payroll_run) and Inventory P2P (purchase_request) landed.
ALTER TABLE operations_approval_policy DROP CONSTRAINT IF EXISTS operations_approval_policy_subject_type_check;
ALTER TABLE operations_approval_policy ADD CONSTRAINT operations_approval_policy_subject_type_check
    CHECK (subject_type IN (
        'expense', 'quote_discount', 'generic',
        'leave_request', 'payroll_run', 'purchase_request'
    ));
ALTER TABLE operations_approval_policy ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_approval_policy FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_approval_policy_tenant_isolation ON operations_approval_policy
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Immutable snapshot of a policy at a given version. Never UPDATE definition_json.
CREATE TABLE IF NOT EXISTS operations_approval_policy_version (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    policy_id       UUID NOT NULL REFERENCES operations_approval_policy(id),
    version         INT NOT NULL,
    -- Routing definition: match criteria, mode, steps, SLA, escalation.
    definition_json JSONB NOT NULL,
    published_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_by    UUID,
    UNIQUE (org_id, policy_id, version)
);
CREATE INDEX IF NOT EXISTS operations_approval_policy_version_org_idx
    ON operations_approval_policy_version (org_id, policy_id, version);
ALTER TABLE operations_approval_policy_version ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_approval_policy_version FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_approval_policy_version_tenant_isolation
    ON operations_approval_policy_version
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Approvals
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_approval (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    subject_type    TEXT NOT NULL,
    -- Opaque public id from the owning context (exp_…, qte_…); never joined.
    subject_id      TEXT NOT NULL,
    subject_uuid    UUID,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending', 'approved', 'rejected', 'cancelled', 'escalated'
    )),
    requester_user_id UUID NOT NULL,
    requester_role  TEXT,
    amount_minor    BIGINT,
    currency        CHAR(3),
    category        TEXT,
    department_id   UUID,
    -- Snapshot of the policy version that routed this approval (permanent).
    policy_id       UUID NOT NULL REFERENCES operations_approval_policy(id),
    policy_version  INT NOT NULL,
    policy_version_id UUID NOT NULL REFERENCES operations_approval_policy_version(id),
    -- Frozen copy of definition + resolved step assignees at request time.
    routing_snapshot JSONB NOT NULL,
    mode            TEXT NOT NULL CHECK (mode IN ('sequential', 'any', 'all')),
    current_step    INT NOT NULL DEFAULT 1,
    title           TEXT NOT NULL,
    summary         TEXT,
    comment         TEXT,
    decided_at      TIMESTAMPTZ,
    decided_by      UUID,
    decision_note   TEXT,
    temporal_workflow_id TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    -- One open approval per subject (idempotent re-request returns existing).
    UNIQUE (org_id, subject_type, subject_id)
);
CREATE INDEX IF NOT EXISTS operations_approval_org_status_idx
    ON operations_approval (org_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS operations_approval_org_requester_idx
    ON operations_approval (org_id, requester_user_id);
ALTER TABLE operations_approval ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_approval FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_approval_tenant_isolation ON operations_approval
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Steps (resolved assignees at request time from the policy snapshot)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_approval_step (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    approval_id     UUID NOT NULL REFERENCES operations_approval(id),
    step_order      INT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending', 'active', 'approved', 'rejected', 'skipped', 'escalated'
    )),
    -- Role name and/or concrete user ids from routing snapshot.
    approver_role   TEXT,
    assignee_user_ids UUID[] NOT NULL DEFAULT '{}',
    sla_seconds     INT,
    escalate_to_role TEXT,
    escalated_at    TIMESTAMPTZ,
    decided_at      TIMESTAMPTZ,
    decided_by      UUID,
    UNIQUE (org_id, approval_id, step_order)
);
CREATE INDEX IF NOT EXISTS operations_approval_step_assignee_idx
    ON operations_approval_step USING GIN (assignee_user_ids);
CREATE INDEX IF NOT EXISTS operations_approval_step_active_idx
    ON operations_approval_step (org_id, status)
    WHERE status = 'active';
ALTER TABLE operations_approval_step ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_approval_step FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_approval_step_tenant_isolation ON operations_approval_step
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Decision audit (append-only; duplicate decide is a no-op at approval level)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_approval_decision (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    approval_id     UUID NOT NULL REFERENCES operations_approval(id),
    step_order      INT,
    decision        TEXT NOT NULL CHECK (decision IN ('approve', 'reject', 'escalate')),
    actor_user_id   UUID NOT NULL,
    on_behalf_of    UUID NOT NULL,
    comment         TEXT,
    idempotency_key TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS operations_approval_decision_idem_idx
    ON operations_approval_decision (org_id, approval_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS operations_approval_decision_org_idx
    ON operations_approval_decision (org_id, approval_id, created_at);
ALTER TABLE operations_approval_decision ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_approval_decision FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_approval_decision_tenant_isolation
    ON operations_approval_decision
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Delegation: approver A delegates to B for a window or a single request
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_approval_delegation (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    from_user_id    UUID NOT NULL,
    to_user_id      UUID NOT NULL,
    -- NULL approval_id → window delegation; set → this-request only.
    approval_id     UUID REFERENCES operations_approval(id),
    starts_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    ends_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at      TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS operations_approval_delegation_org_idx
    ON operations_approval_delegation (org_id, from_user_id, to_user_id)
    WHERE revoked_at IS NULL;
ALTER TABLE operations_approval_delegation ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_approval_delegation FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_approval_delegation_tenant_isolation
    ON operations_approval_delegation
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
