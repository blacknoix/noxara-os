-- Outbox table (shared across services, org_id for tenancy + RLS).
CREATE TABLE IF NOT EXISTS outbox_event (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    subject TEXT NOT NULL,
    payload JSONB NOT NULL,
    idempotency_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS outbox_event_unpublished_idx
    ON outbox_event (created_at)
    WHERE published_at IS NULL;

CREATE INDEX IF NOT EXISTS outbox_event_org_id_idx ON outbox_event (org_id);

ALTER TABLE outbox_event ENABLE ROW LEVEL SECURITY;

ALTER TABLE outbox_event FORCE ROW LEVEL SECURITY;

-- Idempotent: do not DROP POLICY (parallel tests would briefly see deny-all under FORCE RLS).
-- Re-migrate treats duplicate_object (42710) as success in companyos_outbox::migrate.
CREATE POLICY outbox_tenant_isolation ON outbox_event
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
