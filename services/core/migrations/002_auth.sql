-- Phase 1.1 Identity & Authentication
-- Global user_identity — tenant-owned memberships — sessions — refresh rotation — SSO config (flag-gated).

CREATE TABLE IF NOT EXISTS user_identity (
    id UUID PRIMARY KEY,
    public_id TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    email_normalized TEXT NOT NULL UNIQUE,
    email_verified_at TIMESTAMPTZ,
    password_hash TEXT,
    password_salt TEXT,
    display_name TEXT NOT NULL,
    mfa_totp_secret_encrypted TEXT,
    mfa_enabled_at TIMESTAMPTZ,
    failed_login_count INT NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS user_identity_email_normalized_idx
    ON user_identity (email_normalized);

CREATE TABLE IF NOT EXISTS membership (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    user_id UUID NOT NULL REFERENCES user_identity(id),
    public_id TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    policy_version BIGINT NOT NULL DEFAULT 1,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, user_id)
);

CREATE INDEX IF NOT EXISTS membership_org_id_idx ON membership (org_id);
CREATE INDEX IF NOT EXISTS membership_user_id_idx ON membership (user_id);

ALTER TABLE membership ENABLE ROW LEVEL SECURITY;
ALTER TABLE membership FORCE ROW LEVEL SECURITY;

-- Tenant writes/reads when app.org_id is set. Auth login may list a user's
-- memberships across orgs by setting app.auth_lookup_user = user uuid.
CREATE POLICY IF NOT EXISTS membership_tenant_isolation ON membership
    USING (
        org_id = NULLIF(current_setting('app.org_id', true), '')::uuid
        OR user_id = NULLIF(current_setting('app.auth_lookup_user', true), '')::uuid
    )
    WITH CHECK (
        org_id = NULLIF(current_setting('app.org_id', true), '')::uuid
    );

CREATE TABLE IF NOT EXISTS auth_session (
    id UUID PRIMARY KEY,
    family_id UUID NOT NULL,
    user_id UUID NOT NULL REFERENCES user_identity(id),
    org_id UUID NOT NULL REFERENCES organization(id),
    membership_id UUID NOT NULL REFERENCES membership(id),
    device_label TEXT,
    user_agent TEXT,
    ip_address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    revoke_reason TEXT
);

CREATE INDEX IF NOT EXISTS auth_session_user_id_idx ON auth_session (user_id);
CREATE INDEX IF NOT EXISTS auth_session_family_id_idx ON auth_session (family_id);
CREATE INDEX IF NOT EXISTS auth_session_org_user_idx ON auth_session (org_id, user_id);

CREATE TABLE IF NOT EXISTS refresh_token (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES auth_session(id) ON DELETE CASCADE,
    family_id UUID NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    rotated_at TIMESTAMPTZ,
    replaced_by UUID,
    revoked_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS refresh_token_family_id_idx ON refresh_token (family_id);
CREATE INDEX IF NOT EXISTS refresh_token_session_id_idx ON refresh_token (session_id);

CREATE TABLE IF NOT EXISTS mfa_recovery_code (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES user_identity(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS mfa_recovery_code_user_id_idx ON mfa_recovery_code (user_id);

CREATE TABLE IF NOT EXISTS email_token (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES user_identity(id) ON DELETE CASCADE,
    purpose TEXT NOT NULL CHECK (purpose IN (
        'email_verify', 'magic_link', 'password_reset', 'mfa_pending'
    )),
    token_hash TEXT NOT NULL UNIQUE,
    org_id UUID,
    payload JSONB NOT NULL DEFAULT '{}',
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS email_token_user_purpose_idx ON email_token (user_id, purpose);

CREATE TABLE IF NOT EXISTS oauth_account (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES user_identity(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('google', 'microsoft')),
    provider_subject TEXT NOT NULL,
    email TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, provider_subject)
);

CREATE TABLE IF NOT EXISTS oauth_state (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL,
    state_hash TEXT NOT NULL UNIQUE,
    code_verifier TEXT,
    redirect_uri TEXT NOT NULL,
    nonce TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS jwks_signing_key (
    kid TEXT PRIMARY KEY,
    algorithm TEXT NOT NULL DEFAULT 'HS256',
    secret_material TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    retired_at TIMESTAMPTZ,
    is_active BOOLEAN NOT NULL DEFAULT true
);

CREATE TABLE IF NOT EXISTS sso_configuration (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    protocol TEXT NOT NULL CHECK (protocol IN ('saml', 'oidc')),
    display_name TEXT NOT NULL,
    config JSONB NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS sso_configuration_org_id_idx ON sso_configuration (org_id);

ALTER TABLE sso_configuration ENABLE ROW LEVEL SECURITY;
ALTER TABLE sso_configuration FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS sso_tenant_isolation ON sso_configuration
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS auth_idempotency (
    id UUID PRIMARY KEY,
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (scope, key)
);

CREATE TABLE IF NOT EXISTS auth_audit_event (
    id UUID PRIMARY KEY,
    org_id UUID,
    user_id UUID,
    event_type TEXT NOT NULL,
    ip_address TEXT,
    user_agent TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS auth_audit_event_created_at_idx ON auth_audit_event (created_at DESC);
CREATE INDEX IF NOT EXISTS auth_audit_event_user_id_idx ON auth_audit_event (user_id);

-- Org feature flags (SSO disabled unless plan enables)
CREATE TABLE IF NOT EXISTS org_feature_flag (
    org_id UUID NOT NULL REFERENCES organization(id),
    flag TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (org_id, flag)
);
