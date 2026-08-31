-- Phase 3.3 — Outbound org webhooks + API key rate-limit column.
-- Admin-context tables; secrets envelope-encrypted (never logged as plaintext).

-- Per-key rate limit (requests per minute); separate from user-session limits.
ALTER TABLE org_api_key
    ADD COLUMN IF NOT EXISTS rate_limit_per_minute INT NOT NULL DEFAULT 60
        CHECK (rate_limit_per_minute > 0 AND rate_limit_per_minute <= 10000);

-- ---------------------------------------------------------------------------
-- Outbound webhook endpoints
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS webhook_endpoint (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    url TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    event_types JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- Envelope-encrypted signing secret (AES-256-GCM); plaintext shown once.
    secret_ciphertext BYTEA NOT NULL,
    secret_prefix TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'disabled', 'paused')),
    failure_count INT NOT NULL DEFAULT 0,
    last_delivery_at TIMESTAMPTZ,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    disabled_at TIMESTAMPTZ,
    disabled_reason TEXT
);

CREATE INDEX IF NOT EXISTS webhook_endpoint_org_id_idx ON webhook_endpoint (org_id);
CREATE INDEX IF NOT EXISTS webhook_endpoint_org_status_idx
    ON webhook_endpoint (org_id, status);

ALTER TABLE webhook_endpoint ENABLE ROW LEVEL SECURITY;
ALTER TABLE webhook_endpoint FORCE ROW LEVEL SECURITY;

CREATE POLICY webhook_endpoint_tenant_isolation ON webhook_endpoint
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Cross-tenant dispatch path (integration-service sets app.webhook_dispatch=1).
DROP POLICY IF EXISTS webhook_endpoint_dispatch ON webhook_endpoint;
CREATE POLICY webhook_endpoint_dispatch ON webhook_endpoint
    USING (current_setting('app.webhook_dispatch', true) = '1')
    WITH CHECK (current_setting('app.webhook_dispatch', true) = '1');

-- ---------------------------------------------------------------------------
-- Webhook delivery log (at-least-once; retries with backoff)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS webhook_delivery (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    public_id TEXT NOT NULL UNIQUE,
    endpoint_id UUID NOT NULL REFERENCES webhook_endpoint(id),
    -- Idempotent event id so receivers can dedupe; unique per endpoint.
    event_id UUID NOT NULL,
    event_subject TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    attempt INT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'delivering', 'delivered', 'failed', 'dead')),
    status_code INT,
    response_body TEXT,
    delivered_at TIMESTAMPTZ,
    next_retry_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, endpoint_id, event_id)
);

CREATE INDEX IF NOT EXISTS webhook_delivery_org_id_idx ON webhook_delivery (org_id);
CREATE INDEX IF NOT EXISTS webhook_delivery_pending_idx
    ON webhook_delivery (status, next_retry_at)
    WHERE status IN ('pending', 'failed');
CREATE INDEX IF NOT EXISTS webhook_delivery_endpoint_idx
    ON webhook_delivery (org_id, endpoint_id, created_at DESC);

ALTER TABLE webhook_delivery ENABLE ROW LEVEL SECURITY;
ALTER TABLE webhook_delivery FORCE ROW LEVEL SECURITY;

CREATE POLICY webhook_delivery_tenant_isolation ON webhook_delivery
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

DROP POLICY IF EXISTS webhook_delivery_dispatch ON webhook_delivery;
CREATE POLICY webhook_delivery_dispatch ON webhook_delivery
    USING (current_setting('app.webhook_dispatch', true) = '1')
    WITH CHECK (current_setting('app.webhook_dispatch', true) = '1');

-- API request usage counters (feeds analytics via outbox events; OLTP mirror).
CREATE TABLE IF NOT EXISTS api_key_usage (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organization(id),
    api_key_id UUID NOT NULL REFERENCES org_api_key(id),
    route TEXT NOT NULL,
    method TEXT NOT NULL,
    status_code INT NOT NULL,
    duration_ms INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS api_key_usage_org_key_idx
    ON api_key_usage (org_id, api_key_id, created_at DESC);

ALTER TABLE api_key_usage ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_key_usage FORCE ROW LEVEL SECURITY;

CREATE POLICY api_key_usage_tenant_isolation ON api_key_usage
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Cross-tenant hash lookup for gateway API-key exchange (sets app.api_key_lookup=1).
CREATE POLICY org_api_key_hash_lookup ON org_api_key
    FOR SELECT
    USING (current_setting('app.api_key_lookup', true) = '1');
