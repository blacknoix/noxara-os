-- Phase 4.2 — Enterprise multi-tenancy foundations (CMEK, SCIM, hierarchy
-- grants, network allowlist, SLA, eDiscovery / export jobs).
-- No DROP POLICY under FORCE RLS. Postgres 16: no CREATE POLICY IF NOT EXISTS.

-- ---------------------------------------------------------------------------
-- Customer-managed encryption keys (CMEK) — wrap key for per-org DEK
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS org_cmk (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    provider_key_id TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'rotating', 'revoked')),
    created_by      UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at      TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS org_cmk_org_idx ON org_cmk (org_id);
ALTER TABLE org_cmk ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_cmk FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY org_cmk_tenant_isolation ON org_cmk
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- Per-org data encryption key, wrapped by the active CMK
CREATE TABLE IF NOT EXISTS org_data_key (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    cmk_id          UUID NOT NULL REFERENCES org_cmk(id),
    wrapped_dek_b64 TEXT NOT NULL,
    version         INT NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id)
);
CREATE INDEX IF NOT EXISTS org_data_key_org_idx ON org_data_key (org_id);
ALTER TABLE org_data_key ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_data_key FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY org_data_key_tenant_isolation ON org_data_key
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- ---------------------------------------------------------------------------
-- SCIM 2.0 tokens + external id mapping
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS scim_token (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    token_prefix    TEXT NOT NULL,
    token_hash      TEXT NOT NULL,
    idp_label       TEXT NOT NULL DEFAULT 'default',
    created_by      UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at      TIMESTAMPTZ,
    last_used_at    TIMESTAMPTZ,
    UNIQUE (org_id, public_id),
    UNIQUE (token_hash)
);
CREATE INDEX IF NOT EXISTS scim_token_org_idx ON scim_token (org_id);
ALTER TABLE scim_token ENABLE ROW LEVEL SECURITY;
ALTER TABLE scim_token FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY scim_token_tenant_isolation ON scim_token
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- Allow token hash lookup without org session (gateway / SCIM auth)
CREATE TABLE IF NOT EXISTS scim_token_lookup (
    token_hash      TEXT PRIMARY KEY,
    org_id          UUID NOT NULL,
    token_id        UUID NOT NULL,
    revoked_at      TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS scim_external_identity (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    resource_type   TEXT NOT NULL CHECK (resource_type IN ('User', 'Group')),
    external_id     TEXT NOT NULL,
    user_id         UUID,
    team_id         UUID,
    active          BOOLEAN NOT NULL DEFAULT true,
    raw_payload     JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, resource_type, external_id)
);
CREATE INDEX IF NOT EXISTS scim_external_identity_org_idx ON scim_external_identity (org_id);
ALTER TABLE scim_external_identity ENABLE ROW LEVEL SECURITY;
ALTER TABLE scim_external_identity FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY scim_external_identity_tenant_isolation ON scim_external_identity
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- ---------------------------------------------------------------------------
-- Hierarchy inherited grants + membership delegation
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS permission_inherit_grant (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    team_id         UUID NOT NULL REFERENCES team(id),
    permission_id   TEXT NOT NULL,
    effect          TEXT NOT NULL DEFAULT 'allow' CHECK (effect IN ('allow', 'deny')),
    scope           TEXT NOT NULL DEFAULT 'organization',
    created_by      UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, team_id, permission_id, effect)
);
CREATE INDEX IF NOT EXISTS permission_inherit_grant_org_idx ON permission_inherit_grant (org_id);
CREATE INDEX IF NOT EXISTS permission_inherit_grant_team_idx ON permission_inherit_grant (org_id, team_id);
ALTER TABLE permission_inherit_grant ENABLE ROW LEVEL SECURITY;
ALTER TABLE permission_inherit_grant FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY permission_inherit_grant_tenant_isolation ON permission_inherit_grant
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

CREATE TABLE IF NOT EXISTS permission_delegation (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    from_membership_id  UUID NOT NULL REFERENCES membership(id),
    to_membership_id    UUID NOT NULL REFERENCES membership(id),
    permission_id       TEXT NOT NULL,
    scope               TEXT NOT NULL DEFAULT 'organization',
    expires_at          TIMESTAMPTZ NOT NULL,
    created_by          UUID NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at          TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS permission_delegation_org_idx ON permission_delegation (org_id);
CREATE INDEX IF NOT EXISTS permission_delegation_to_idx
    ON permission_delegation (org_id, to_membership_id) WHERE revoked_at IS NULL;
ALTER TABLE permission_delegation ENABLE ROW LEVEL SECURITY;
ALTER TABLE permission_delegation FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY permission_delegation_tenant_isolation ON permission_delegation
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- ---------------------------------------------------------------------------
-- Network allowlist + infrastructure tier (config only; fail-closed in gateway)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS org_network_policy (
    org_id              UUID PRIMARY KEY REFERENCES organization(id),
    infra_tier          TEXT NOT NULL DEFAULT 'shared'
                        CHECK (infra_tier IN ('shared', 'dedicated')),
    allowlist_enabled   BOOLEAN NOT NULL DEFAULT false,
    cidr_allowlist      TEXT[] NOT NULL DEFAULT '{}',
    mtls_client_ids     TEXT[] NOT NULL DEFAULT '{}',
    updated_by          UUID,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INT NOT NULL DEFAULT 1
);
ALTER TABLE org_network_policy ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_network_policy FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY org_network_policy_tenant_isolation ON org_network_policy
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- Non-RLS mirror for gateway fail-closed checks (org_id from token)
CREATE TABLE IF NOT EXISTS org_network_policy_lookup (
    org_id              UUID PRIMARY KEY,
    infra_tier          TEXT NOT NULL DEFAULT 'shared',
    allowlist_enabled   BOOLEAN NOT NULL DEFAULT false,
    cidr_allowlist      TEXT[] NOT NULL DEFAULT '{}',
    mtls_client_ids     TEXT[] NOT NULL DEFAULT '{}',
    version             INT NOT NULL DEFAULT 1
);

-- ---------------------------------------------------------------------------
-- Per-tenant SLA targets
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS org_sla_target (
    org_id                  UUID PRIMARY KEY REFERENCES organization(id),
    availability_pct_bps    INT NOT NULL DEFAULT 9990,  -- 99.90%
    latency_p99_ms          INT NOT NULL DEFAULT 500,
    updated_by              UUID,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE org_sla_target ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_sla_target FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY org_sla_target_tenant_isolation ON org_sla_target
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- ---------------------------------------------------------------------------
-- Legal hold + durable export jobs (eDiscovery)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS legal_hold (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    reason          TEXT NOT NULL,
    active          BOOLEAN NOT NULL DEFAULT true,
    created_by      UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    released_at     TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS legal_hold_org_idx ON legal_hold (org_id);
ALTER TABLE legal_hold ENABLE ROW LEVEL SECURITY;
ALTER TABLE legal_hold FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY legal_hold_tenant_isolation ON legal_hold
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

CREATE TABLE IF NOT EXISTS export_job (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('ediscovery', 'audit', 'consolidation')),
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'running', 'completed', 'failed', 'expired')),
    include_contexts TEXT[] NOT NULL DEFAULT '{audit}',
    legal_hold_id   UUID REFERENCES legal_hold(id),
    file_public_id  TEXT,
    file_bytes      BYTEA,
    content_type    TEXT,
    hash_chain_ok   BOOLEAN,
    error_message   TEXT,
    created_by      UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at    TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS export_job_org_idx ON export_job (org_id);
ALTER TABLE export_job ENABLE ROW LEVEL SECURITY;
ALTER TABLE export_job FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
    CREATE POLICY export_job_tenant_isolation ON export_job
        USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
        WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
