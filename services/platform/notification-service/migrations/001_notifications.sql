-- Phase 1.8 notification service schema.

CREATE TABLE IF NOT EXISTS notification_preference (
    org_id UUID NOT NULL,
    user_id UUID NOT NULL,
    channel TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    quiet_hours_start TIMETZ,
    quiet_hours_end TIMETZ,
    digest_cron TEXT,
    PRIMARY KEY (org_id, user_id, channel)
);

CREATE INDEX IF NOT EXISTS notification_preference_user_idx
    ON notification_preference (org_id, user_id);

ALTER TABLE notification_preference ENABLE ROW LEVEL SECURITY;
ALTER TABLE notification_preference FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS notification_preference_tenant_isolation ON notification_preference
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- System templates use org_id NULL; org overrides use org_id set.
CREATE TABLE IF NOT EXISTS notification_template (
    id UUID PRIMARY KEY,
    org_id UUID,
    key TEXT NOT NULL,
    channel TEXT NOT NULL,
    subject_template TEXT NOT NULL,
    body_template TEXT NOT NULL,
    UNIQUE (org_id, key, channel)
);

CREATE INDEX IF NOT EXISTS notification_template_key_idx
    ON notification_template (key, channel);

CREATE TABLE IF NOT EXISTS notification_item (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    user_id UUID NOT NULL,
    public_id TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    href TEXT,
    resource_type TEXT,
    resource_id TEXT,
    read_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS notification_item_feed_idx
    ON notification_item (org_id, user_id, created_at DESC);

ALTER TABLE notification_item ENABLE ROW LEVEL SECURITY;
ALTER TABLE notification_item FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS notification_item_tenant_isolation ON notification_item
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS notification_delivery (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    item_id UUID NOT NULL REFERENCES notification_item(id),
    channel TEXT NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS notification_delivery_item_idx
    ON notification_delivery (org_id, item_id);

ALTER TABLE notification_delivery ENABLE ROW LEVEL SECURITY;
ALTER TABLE notification_delivery FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS notification_delivery_tenant_isolation ON notification_delivery
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS notification_processed (
    idempotency_key TEXT PRIMARY KEY,
    org_id UUID NOT NULL,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE notification_processed ENABLE ROW LEVEL SECURITY;
ALTER TABLE notification_processed FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS notification_processed_tenant_isolation ON notification_processed
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Cross-tenant ingest/consumer path (service sets app.notification_ingest=1).
CREATE POLICY IF NOT EXISTS notification_processed_ingest ON notification_processed
    USING (current_setting('app.notification_ingest', true) = '1')
    WITH CHECK (current_setting('app.notification_ingest', true) = '1');

CREATE POLICY IF NOT EXISTS notification_item_ingest ON notification_item
    USING (current_setting('app.notification_ingest', true) = '1')
    WITH CHECK (current_setting('app.notification_ingest', true) = '1');

CREATE POLICY IF NOT EXISTS notification_delivery_ingest ON notification_delivery
    USING (current_setting('app.notification_ingest', true) = '1')
    WITH CHECK (current_setting('app.notification_ingest', true) = '1');

CREATE POLICY IF NOT EXISTS notification_preference_ingest ON notification_preference
    USING (current_setting('app.notification_ingest', true) = '1')
    WITH CHECK (current_setting('app.notification_ingest', true) = '1');
