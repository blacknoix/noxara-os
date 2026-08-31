-- Phase 3.4 — Marketplace skeleton (listings, review, installs, OAuth clients, app tokens).
--
-- Every table is org-scoped with FORCE ROW LEVEL SECURITY. Policies are created
-- with plain CREATE POLICY (no DROP POLICY): dropping a policy under FORCE RLS
-- briefly denies concurrent tenant traffic. `execute_migration_stmt` swallows
-- duplicate_object (42710) so re-running the migration is a no-op.
--
-- Tenancy model:
--   * marketplace_listing / marketplace_oauth_client / marketplace_review are
--     owned by the PUBLISHER org. Published listings are readable by every org
--     (that is the catalogue).
--   * marketplace_install / marketplace_app_token / marketplace_oauth_code are
--     owned by the INSTALLING (customer) org — strict tenant isolation.

-- ---------------------------------------------------------------------------
-- Listings (publisher org owns the row)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS marketplace_listing (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    listing_kind TEXT NOT NULL DEFAULT 'third_party'
        CHECK (listing_kind IN ('first_party', 'third_party')),
    connector_key TEXT,
    requested_scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    redirect_uris JSONB NOT NULL DEFAULT '[]'::jsonb,
    webhook_subscriptions JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'submitted', 'in_review', 'approved', 'rejected', 'published', 'suspended')),
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, slug)
);

CREATE INDEX IF NOT EXISTS marketplace_listing_org_idx
    ON marketplace_listing (org_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS marketplace_listing_status_idx
    ON marketplace_listing (status, updated_at DESC);

-- One canonical published listing per connector key across the catalogue.
CREATE UNIQUE INDEX IF NOT EXISTS marketplace_listing_connector_published_uniq
    ON marketplace_listing (connector_key)
    WHERE connector_key IS NOT NULL AND status = 'published';

ALTER TABLE marketplace_listing ENABLE ROW LEVEL SECURITY;
ALTER TABLE marketplace_listing FORCE ROW LEVEL SECURITY;

CREATE POLICY marketplace_listing_tenant ON marketplace_listing
    USING (
        org_id = NULLIF(current_setting('app.org_id', true), '')::uuid
        OR status = 'published'
    )
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE POLICY marketplace_listing_seed ON marketplace_listing
    USING (current_setting('app.marketplace_seed', true) = '1')
    WITH CHECK (current_setting('app.marketplace_seed', true) = '1');

-- ---------------------------------------------------------------------------
-- OAuth clients (publisher org owns the row)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS marketplace_oauth_client (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    listing_id UUID NOT NULL REFERENCES marketplace_listing(id) ON DELETE CASCADE,
    public_id TEXT NOT NULL UNIQUE,
    client_id TEXT NOT NULL UNIQUE,
    client_secret_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS marketplace_oauth_client_listing_idx
    ON marketplace_oauth_client (listing_id);

ALTER TABLE marketplace_oauth_client ENABLE ROW LEVEL SECURITY;
ALTER TABLE marketplace_oauth_client FORCE ROW LEVEL SECURITY;

CREATE POLICY marketplace_oauth_client_tenant ON marketplace_oauth_client
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Token exchange authenticates client_id + client_secret before any org is known.
CREATE POLICY marketplace_oauth_client_lookup ON marketplace_oauth_client
    USING (current_setting('app.marketplace_token_lookup', true) = '1')
    WITH CHECK (current_setting('app.marketplace_token_lookup', true) = '1');

CREATE POLICY marketplace_oauth_client_seed ON marketplace_oauth_client
    USING (current_setting('app.marketplace_seed', true) = '1')
    WITH CHECK (current_setting('app.marketplace_seed', true) = '1');

-- ---------------------------------------------------------------------------
-- Review record (publisher org owns the row; reviewers hold admin.marketplace.review)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS marketplace_review (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    listing_id UUID NOT NULL UNIQUE REFERENCES marketplace_listing(id) ON DELETE CASCADE,
    public_id TEXT NOT NULL UNIQUE,
    checklist JSONB NOT NULL DEFAULT '[]'::jsonb,
    security_review_completed BOOLEAN NOT NULL DEFAULT false,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'in_review', 'approved', 'rejected', 'published')),
    reviewer_notes TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS marketplace_review_org_idx
    ON marketplace_review (org_id, updated_at DESC);

ALTER TABLE marketplace_review ENABLE ROW LEVEL SECURITY;
ALTER TABLE marketplace_review FORCE ROW LEVEL SECURITY;

CREATE POLICY marketplace_review_tenant ON marketplace_review
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE POLICY marketplace_review_seed ON marketplace_review
    USING (current_setting('app.marketplace_seed', true) = '1')
    WITH CHECK (current_setting('app.marketplace_seed', true) = '1');

-- ---------------------------------------------------------------------------
-- Installs (installing / customer org owns the row — strict isolation)
-- ---------------------------------------------------------------------------
-- listing_id has no FK: the listing lives in the publisher org and may be
-- suspended or removed independently. Denormalised listing columns keep the
-- install renderable without a cross-org join.
CREATE TABLE IF NOT EXISTS marketplace_install (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL UNIQUE,
    listing_id UUID NOT NULL,
    listing_public_id TEXT NOT NULL,
    listing_slug TEXT NOT NULL DEFAULT '',
    listing_name TEXT NOT NULL DEFAULT '',
    listing_kind TEXT NOT NULL DEFAULT 'third_party',
    connector_key TEXT,
    consented_scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'revoked')),
    installed_by UUID NOT NULL,
    installed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    revoked_by UUID,
    outbound_enabled BOOLEAN NOT NULL DEFAULT true,
    inbound_enabled BOOLEAN NOT NULL DEFAULT true,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS marketplace_install_org_idx
    ON marketplace_install (org_id, installed_at DESC);

CREATE INDEX IF NOT EXISTS marketplace_install_connector_idx
    ON marketplace_install (org_id, connector_key);

CREATE UNIQUE INDEX IF NOT EXISTS marketplace_install_active_uniq
    ON marketplace_install (org_id, listing_id)
    WHERE status = 'active';

ALTER TABLE marketplace_install ENABLE ROW LEVEL SECURITY;
ALTER TABLE marketplace_install FORCE ROW LEVEL SECURITY;

CREATE POLICY marketplace_install_tenant ON marketplace_install
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- App tokens (installing org owns the row)
-- ---------------------------------------------------------------------------
-- `scopes` MUST equal the install's consented_scopes at issue time. Widening
-- consent revokes and re-issues rather than mutating a live token.
CREATE TABLE IF NOT EXISTS marketplace_app_token (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    install_id UUID NOT NULL REFERENCES marketplace_install(id) ON DELETE CASCADE,
    public_id TEXT NOT NULL UNIQUE,
    token_kind TEXT NOT NULL CHECK (token_kind IN ('access', 'refresh')),
    token_hash TEXT NOT NULL,
    token_prefix TEXT NOT NULL DEFAULT '',
    scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS marketplace_app_token_hash_idx
    ON marketplace_app_token (token_hash);

CREATE INDEX IF NOT EXISTS marketplace_app_token_install_idx
    ON marketplace_app_token (install_id, token_kind);

ALTER TABLE marketplace_app_token ENABLE ROW LEVEL SECURITY;
ALTER TABLE marketplace_app_token FORCE ROW LEVEL SECURITY;

CREATE POLICY marketplace_app_token_tenant ON marketplace_app_token
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Bearer-token authorization resolves the org from the token hash.
CREATE POLICY marketplace_app_token_lookup ON marketplace_app_token
    USING (current_setting('app.marketplace_token_lookup', true) = '1')
    WITH CHECK (current_setting('app.marketplace_token_lookup', true) = '1');

-- ---------------------------------------------------------------------------
-- Authorization codes (PKCE) — installing org owns the row
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS marketplace_oauth_code (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    listing_id UUID NOT NULL,
    install_id UUID,
    code_hash TEXT NOT NULL UNIQUE,
    code_challenge TEXT NOT NULL DEFAULT '',
    code_challenge_method TEXT NOT NULL DEFAULT 'S256',
    redirect_uri TEXT NOT NULL,
    consented_scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS marketplace_oauth_code_org_idx
    ON marketplace_oauth_code (org_id, created_at DESC);

ALTER TABLE marketplace_oauth_code ENABLE ROW LEVEL SECURITY;
ALTER TABLE marketplace_oauth_code FORCE ROW LEVEL SECURITY;

CREATE POLICY marketplace_oauth_code_tenant ON marketplace_oauth_code
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Code exchange happens on the unauthenticated token endpoint.
CREATE POLICY marketplace_oauth_code_lookup ON marketplace_oauth_code
    USING (current_setting('app.marketplace_token_lookup', true) = '1')
    WITH CHECK (current_setting('app.marketplace_token_lookup', true) = '1');
