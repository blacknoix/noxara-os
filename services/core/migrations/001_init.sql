-- Phase 0 core schema: organizations, users (seed), hello_message, outbox, audit

CREATE TABLE IF NOT EXISTS organization (
    id UUID PRIMARY KEY,
    public_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS app_user (
    id UUID PRIMARY KEY,
    public_id TEXT NOT NULL UNIQUE,
    org_id UUID NOT NULL REFERENCES organization(id),
    email TEXT NOT NULL,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'member')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS app_user_org_id_idx ON app_user (org_id);

CREATE TABLE IF NOT EXISTS hello_message (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL UNIQUE,
    message TEXT NOT NULL,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS hello_message_org_id_idx ON hello_message (org_id);

ALTER TABLE hello_message ENABLE ROW LEVEL SECURITY;
ALTER TABLE hello_message FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS hello_tenant_isolation ON hello_message;
CREATE POLICY hello_tenant_isolation ON hello_message
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS audit_entry (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    actor_user_id UUID NOT NULL,
    actor_on_behalf_of UUID NOT NULL,
    actor_is_ai BOOLEAN NOT NULL DEFAULT false,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS audit_entry_org_id_idx ON audit_entry (org_id);

ALTER TABLE audit_entry ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_entry FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS audit_tenant_isolation ON audit_entry;
CREATE POLICY audit_tenant_isolation ON audit_entry
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Outbox (shared)
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

DROP POLICY IF EXISTS outbox_tenant_isolation ON outbox_event;
CREATE POLICY outbox_tenant_isolation ON outbox_event
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
