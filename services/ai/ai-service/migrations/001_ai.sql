-- Phase 1.9 AI assistant schema (own tables; RLS on org_id).

CREATE TABLE IF NOT EXISTS ai_org_settings (
    org_id UUID PRIMARY KEY,
    modules_enabled JSONB NOT NULL DEFAULT '{"copilot":true,"insights":true,"document_ai":true,"ask_mode":true}'::jsonb,
    model_preference TEXT NOT NULL DEFAULT 'mock',
    auto_execute_allow_list JSONB NOT NULL DEFAULT '[]'::jsonb,
    data_sharing JSONB NOT NULL DEFAULT '{"share_with_provider":false,"allow_training":false}'::jsonb,
    monthly_token_budget BIGINT NOT NULL DEFAULT 500000,
    tokens_used_this_month BIGINT NOT NULL DEFAULT 0,
    budget_month TEXT NOT NULL DEFAULT to_char(now() AT TIME ZONE 'utc', 'YYYY-MM'),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ai_org_settings ENABLE ROW LEVEL SECURITY;
CREATE POLICY ai_org_settings_tenant ON ai_org_settings
    USING (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_session (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    user_id UUID NOT NULL,
    title TEXT NOT NULL DEFAULT 'Copilot',
    page_scope TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS ai_session_org_user_idx ON ai_session (org_id, user_id, updated_at DESC);

ALTER TABLE ai_session ENABLE ROW LEVEL SECURITY;
CREATE POLICY ai_session_tenant ON ai_session
    USING (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_interaction (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    session_id UUID NOT NULL REFERENCES ai_session(id) ON DELETE CASCADE,
    user_id UUID NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    citations JSONB NOT NULL DEFAULT '[]'::jsonb,
    follow_ups JSONB NOT NULL DEFAULT '[]'::jsonb,
    tool_trace JSONB NOT NULL DEFAULT '[]'::jsonb,
    model TEXT,
    prompt_template_version TEXT,
    input_tokens INT NOT NULL DEFAULT 0,
    output_tokens INT NOT NULL DEFAULT 0,
    latency_ms INT NOT NULL DEFAULT 0,
    cost_estimate_minor BIGINT NOT NULL DEFAULT 0,
    currency TEXT NOT NULL DEFAULT 'USD',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS ai_interaction_session_idx ON ai_interaction (org_id, session_id, created_at);

ALTER TABLE ai_interaction ENABLE ROW LEVEL SECURITY;
CREATE POLICY ai_interaction_tenant ON ai_interaction
    USING (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_proposal (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    user_id UUID NOT NULL,
    interaction_id UUID,
    tool_name TEXT NOT NULL,
    action_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    command JSONB NOT NULL,
    rendered_diff TEXT NOT NULL,
    citations JSONB NOT NULL DEFAULT '[]'::jsonb,
    domain_path TEXT,
    domain_method TEXT,
    domain_body JSONB,
    result JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS ai_proposal_org_user_idx ON ai_proposal (org_id, user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS ai_proposal_status_idx ON ai_proposal (org_id, status);

ALTER TABLE ai_proposal ENABLE ROW LEVEL SECURITY;
CREATE POLICY ai_proposal_tenant ON ai_proposal
    USING (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_document_review (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    user_id UUID NOT NULL,
    file_id TEXT,
    kind TEXT NOT NULL DEFAULT 'expense',
    extractor TEXT NOT NULL DEFAULT 'stub',
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    extracted JSONB NOT NULL DEFAULT '{}'::jsonb,
    proposal_id UUID,
    status TEXT NOT NULL DEFAULT 'pending_review',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ai_document_review ENABLE ROW LEVEL SECURITY;
CREATE POLICY ai_document_review_tenant ON ai_document_review
    USING (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.current_org_id', true), '')::uuid);
