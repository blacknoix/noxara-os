-- Phase 1.11 — push device token registration (no live FCM/APNs delivery).

CREATE TABLE IF NOT EXISTS notification_device (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    user_id UUID NOT NULL,
    public_id TEXT NOT NULL UNIQUE,
    platform TEXT NOT NULL,
    push_token TEXT NOT NULL,
    device_label TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, user_id, push_token)
);

CREATE INDEX IF NOT EXISTS notification_device_user_idx
    ON notification_device (org_id, user_id);

ALTER TABLE notification_device ENABLE ROW LEVEL SECURITY;
ALTER TABLE notification_device FORCE ROW LEVEL SECURITY;

CREATE POLICY notification_device_tenant_isolation ON notification_device
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
