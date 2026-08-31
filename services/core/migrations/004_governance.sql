-- Phase 2.6 — Security & governance hardening
-- Access review history, retention, API keys, audit hash-chain, ABAC conditions.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ---------------------------------------------------------------------------
-- Audit hash chain (append-only partitions)
-- ---------------------------------------------------------------------------
ALTER TABLE audit_entry
    ADD COLUMN IF NOT EXISTS partition_key TEXT,
    ADD COLUMN IF NOT EXISTS prev_hash TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS content_hash TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS audit_entry_partition_idx
    ON audit_entry (org_id, partition_key, created_at, id);

CREATE INDEX IF NOT EXISTS audit_entry_action_created_idx
    ON audit_entry (org_id, action, created_at DESC);

CREATE OR REPLACE FUNCTION companyos_audit_hash_chain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  v_prev text := '';
  v_lock_key bigint;
  v_payload text;
BEGIN
  NEW.partition_key := to_char(COALESCE(NEW.created_at, now()) AT TIME ZONE 'UTC', 'YYYY-MM');
  -- Serialize writers per org+partition so the chain stays linear.
  v_lock_key := hashtextextended(NEW.org_id::text || ':' || NEW.partition_key, 0);
  PERFORM pg_advisory_xact_lock(v_lock_key);

  SELECT COALESCE(content_hash, '') INTO v_prev
  FROM audit_entry
  WHERE org_id = NEW.org_id
    AND partition_key = NEW.partition_key
  ORDER BY created_at DESC, id DESC
  LIMIT 1;

  NEW.prev_hash := COALESCE(v_prev, '');
  v_payload := COALESCE(NEW.prev_hash, '')
    || '|' || NEW.id::text
    || '|' || NEW.org_id::text
    || '|' || NEW.actor_user_id::text
    || '|' || NEW.actor_on_behalf_of::text
    || '|' || (CASE WHEN NEW.actor_is_ai THEN '1' ELSE '0' END)
    || '|' || NEW.action
    || '|' || NEW.resource_type
    || '|' || NEW.resource_id
    || '|' || COALESCE(NEW.metadata::text, '{}')
    || '|' || COALESCE(NEW.created_at, now())::text;
  NEW.content_hash := encode(digest(v_payload, 'sha256'), 'hex');
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS audit_entry_hash_chain_trg ON audit_entry;
CREATE TRIGGER audit_entry_hash_chain_trg
    BEFORE INSERT ON audit_entry
    FOR EACH ROW
    EXECUTE FUNCTION companyos_audit_hash_chain();

-- ---------------------------------------------------------------------------
-- ABAC conditions on role grants
-- ---------------------------------------------------------------------------
ALTER TABLE role_permission
    ADD COLUMN IF NOT EXISTS conditions JSONB NOT NULL DEFAULT '[]'::jsonb;

-- ---------------------------------------------------------------------------
-- Permission entitlement history (answer "who could see X in period Y")
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS permission_entitlement_history (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    user_id UUID NOT NULL,
    membership_id UUID NOT NULL,
    role_key TEXT NOT NULL,
    permission_id TEXT NOT NULL,
    effect TEXT NOT NULL CHECK (effect IN ('allow', 'deny')),
    effective_from TIMESTAMPTZ NOT NULL,
    effective_to TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS permission_entitlement_history_lookup_idx
    ON permission_entitlement_history (org_id, permission_id, effective_from, effective_to);

CREATE INDEX IF NOT EXISTS permission_entitlement_history_user_idx
    ON permission_entitlement_history (org_id, user_id);

CREATE INDEX IF NOT EXISTS permission_entitlement_history_org_id_idx
    ON permission_entitlement_history (org_id);

ALTER TABLE permission_entitlement_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE permission_entitlement_history FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS permission_entitlement_history_tenant_isolation ON permission_entitlement_history;
CREATE POLICY permission_entitlement_history_tenant_isolation ON permission_entitlement_history
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Access review runs + exportable findings
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS access_review_run (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'completed'
        CHECK (status IN ('pending', 'completed', 'failed')),
    permission_id TEXT NOT NULL,
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    summary JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS access_review_run_org_id_idx ON access_review_run (org_id);

ALTER TABLE access_review_run ENABLE ROW LEVEL SECURITY;
ALTER TABLE access_review_run FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS access_review_run_tenant_isolation ON access_review_run;
CREATE POLICY access_review_run_tenant_isolation ON access_review_run
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS access_review_finding (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    run_id UUID NOT NULL REFERENCES access_review_run(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('could_see', 'did_see', 'role_summary')),
    user_id UUID,
    role_key TEXT,
    permission_id TEXT NOT NULL,
    detail JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS access_review_finding_run_idx ON access_review_finding (org_id, run_id);
CREATE INDEX IF NOT EXISTS access_review_finding_org_id_idx ON access_review_finding (org_id);

ALTER TABLE access_review_finding ENABLE ROW LEVEL SECURITY;
ALTER TABLE access_review_finding FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS access_review_finding_tenant_isolation ON access_review_finding;
CREATE POLICY access_review_finding_tenant_isolation ON access_review_finding
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Per-org retention configuration
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS org_retention_config (
    org_id UUID PRIMARY KEY REFERENCES organization(id),
    default_retention_days INT NOT NULL DEFAULT 2555
        CHECK (default_retention_days >= 30 AND default_retention_days <= 3650),
    overrides JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_by UUID,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    version INT NOT NULL DEFAULT 1
);

ALTER TABLE org_retention_config ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_retention_config FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS org_retention_config_tenant_isolation ON org_retention_config;
CREATE POLICY org_retention_config_tenant_isolation ON org_retention_config
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Organization API keys (hashed at rest; rotation supported)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS org_api_key (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    rotated_from UUID REFERENCES org_api_key(id),
    revoked_at TIMESTAMPTZ,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS org_api_key_org_id_idx ON org_api_key (org_id);
CREATE INDEX IF NOT EXISTS org_api_key_hash_idx ON org_api_key (key_hash)
    WHERE revoked_at IS NULL;

ALTER TABLE org_api_key ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_api_key FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS org_api_key_tenant_isolation ON org_api_key;
CREATE POLICY org_api_key_tenant_isolation ON org_api_key
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Org secrets / integration tokens (hashed; rotation automation)
CREATE TABLE IF NOT EXISTS org_secret (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    secret_kind TEXT NOT NULL DEFAULT 'integration_token',
    secret_hash TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    rotated_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS org_secret_org_id_idx ON org_secret (org_id);

ALTER TABLE org_secret ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_secret FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS org_secret_tenant_isolation ON org_secret;
CREATE POLICY org_secret_tenant_isolation ON org_secret
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Governance idempotency + SSO login state
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS governance_idempotency (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, scope, key)
);

CREATE INDEX IF NOT EXISTS governance_idempotency_org_id_idx ON governance_idempotency (org_id);

ALTER TABLE governance_idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE governance_idempotency FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS governance_idempotency_tenant_isolation ON governance_idempotency;
CREATE POLICY governance_idempotency_tenant_isolation ON governance_idempotency
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS sso_login_state (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    sso_config_id UUID NOT NULL REFERENCES sso_configuration(id) ON DELETE CASCADE,
    state_hash TEXT NOT NULL UNIQUE,
    code_verifier TEXT NOT NULL,
    nonce TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS sso_login_state_org_id_idx ON sso_login_state (org_id);

ALTER TABLE sso_login_state ENABLE ROW LEVEL SECURITY;
ALTER TABLE sso_login_state FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS sso_login_state_tenant_isolation ON sso_login_state;
CREATE POLICY sso_login_state_tenant_isolation ON sso_login_state
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Link IdP subject → user (SSO; no god-account bypass)
CREATE TABLE IF NOT EXISTS sso_identity_link (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    sso_config_id UUID NOT NULL REFERENCES sso_configuration(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES user_identity(id),
    idp_subject TEXT NOT NULL,
    email TEXT,
    linked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, sso_config_id, idp_subject)
);

CREATE INDEX IF NOT EXISTS sso_identity_link_org_id_idx ON sso_identity_link (org_id);
CREATE INDEX IF NOT EXISTS sso_identity_link_user_idx ON sso_identity_link (user_id);

ALTER TABLE sso_identity_link ENABLE ROW LEVEL SECURITY;
ALTER TABLE sso_identity_link FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS sso_identity_link_tenant_isolation ON sso_identity_link;
CREATE POLICY sso_identity_link_tenant_isolation ON sso_identity_link
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
