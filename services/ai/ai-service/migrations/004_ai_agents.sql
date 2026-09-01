-- Phase 4.3 — Autonomous agents: policy, runs, ai_action, kill switch, prompt packs.
-- Every tenant-owned row carries org_id; FORCE RLS via app.org_id.
-- PG16: no CREATE POLICY IF NOT EXISTS — re-runs tolerate duplicate_object (42710).
-- No DROP POLICY under FORCE RLS.

-- Versioned org agent policy (in-flight runs pin the version they started with).
CREATE TABLE IF NOT EXISTS ai_agent_policy (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    version INT NOT NULL,
    name TEXT NOT NULL DEFAULT 'default',
    status TEXT NOT NULL DEFAULT 'active',
    agent_types JSONB NOT NULL DEFAULT '["receivables_chase"]'::jsonb,
    allowed_tools JSONB NOT NULL DEFAULT '[]'::jsonb,
    allowed_permissions JSONB NOT NULL DEFAULT '[]'::jsonb,
    spend_budget_tokens BIGINT NOT NULL DEFAULT 100000,
    max_steps INT NOT NULL DEFAULT 50,
    require_human_above JSONB NOT NULL DEFAULT '{}'::jsonb,
    allowed_resource_scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, version),
    UNIQUE (org_id, public_id)
);

CREATE INDEX IF NOT EXISTS ai_agent_policy_org_status_idx
    ON ai_agent_policy (org_id, status, version DESC);

ALTER TABLE ai_agent_policy ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_agent_policy FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_agent_policy_tenant ON ai_agent_policy
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_agent_kill_switch (
    org_id UUID NOT NULL,
    agent_type TEXT NOT NULL DEFAULT '*',
    engaged BOOLEAN NOT NULL DEFAULT false,
    engaged_by UUID,
    engaged_at TIMESTAMPTZ,
    reason TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, agent_type)
);

ALTER TABLE ai_agent_kill_switch ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_agent_kill_switch FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_agent_kill_switch_tenant ON ai_agent_kill_switch
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_agent_run (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    agent_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    policy_id UUID NOT NULL,
    policy_version INT NOT NULL,
    on_behalf_of UUID,
    scheduled_policy BOOLEAN NOT NULL DEFAULT false,
    temporal_workflow_id TEXT NOT NULL,
    steps_taken INT NOT NULL DEFAULT 0,
    tokens_used INT NOT NULL DEFAULT 0,
    cost_estimate_minor BIGINT NOT NULL DEFAULT 0,
    last_actions JSONB NOT NULL DEFAULT '[]'::jsonb,
    error_message TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);

CREATE INDEX IF NOT EXISTS ai_agent_run_org_status_idx
    ON ai_agent_run (org_id, status, started_at DESC);

ALTER TABLE ai_agent_run ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_agent_run FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_agent_run_tenant ON ai_agent_run
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_action (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    run_id UUID,
    agent_type TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    permission TEXT NOT NULL,
    model TEXT NOT NULL DEFAULT 'mock',
    prompt_template_version TEXT NOT NULL DEFAULT 'ai.agent.v1',
    tool_trace JSONB NOT NULL DEFAULT '[]'::jsonb,
    command JSONB NOT NULL DEFAULT '{}'::jsonb,
    effect JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'committed',
    reversible BOOLEAN NOT NULL DEFAULT true,
    reversibility_window_secs INT NOT NULL DEFAULT 86400,
    reversed_at TIMESTAMPTZ,
    reverse_of UUID,
    error BOOLEAN NOT NULL DEFAULT false,
    error_message TEXT,
    on_behalf_of UUID,
    policy_version INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);

CREATE INDEX IF NOT EXISTS ai_action_org_created_idx
    ON ai_action (org_id, created_at DESC);
CREATE INDEX IF NOT EXISTS ai_action_org_run_idx
    ON ai_action (org_id, run_id);
CREATE INDEX IF NOT EXISTS ai_action_org_status_idx
    ON ai_action (org_id, status, error);

ALTER TABLE ai_action ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_action FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_action_tenant ON ai_action
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_agent_effect (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    action_id UUID NOT NULL,
    effect_type TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    reversed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS ai_agent_effect_action_idx
    ON ai_agent_effect (org_id, action_id);

ALTER TABLE ai_agent_effect ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_agent_effect FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_agent_effect_tenant ON ai_agent_effect
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_tenant_prompt_pack (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT 'default',
    allowed_models JSONB NOT NULL DEFAULT '["mock"]'::jsonb,
    temperature DOUBLE PRECISION NOT NULL DEFAULT 0.2,
    tool_subset JSONB NOT NULL DEFAULT '[]'::jsonb,
    system_preamble TEXT NOT NULL DEFAULT '',
    active BOOLEAN NOT NULL DEFAULT true,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);

ALTER TABLE ai_tenant_prompt_pack ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_tenant_prompt_pack FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_tenant_prompt_pack_tenant ON ai_tenant_prompt_pack
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_workflow_draft (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    user_id UUID NOT NULL,
    prompt TEXT NOT NULL,
    definition JSONB NOT NULL,
    filtered_actions JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'draft',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ai_workflow_draft ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_workflow_draft FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_workflow_draft_tenant ON ai_workflow_draft
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_agent_review_threshold (
    org_id UUID PRIMARY KEY,
    max_error_rate DOUBLE PRECISION NOT NULL DEFAULT 0.05,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ai_agent_review_threshold ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_agent_review_threshold FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_agent_review_threshold_tenant ON ai_agent_review_threshold
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
