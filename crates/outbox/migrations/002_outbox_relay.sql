-- Phase 1.8: relay cross-tenant publish + DLQ.
-- Relay sets `app.outbox_relay = '1'` (bypasses tenant session) so it can
-- publish all orgs' unpublished rows. Permissive policies OR together.

DROP POLICY IF EXISTS outbox_relay_publish ON outbox_event;
CREATE POLICY outbox_relay_publish ON outbox_event
    USING (current_setting('app.outbox_relay', true) = '1')
    WITH CHECK (current_setting('app.outbox_relay', true) = '1');

CREATE TABLE IF NOT EXISTS outbox_dlq (
    id UUID PRIMARY KEY,
    outbox_id UUID NOT NULL,
    org_id UUID NOT NULL,
    subject TEXT NOT NULL,
    payload JSONB NOT NULL,
    idempotency_key TEXT NOT NULL,
    error TEXT NOT NULL,
    attempts INT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    replayed_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS outbox_dlq_unreplayed_idx
    ON outbox_dlq (created_at)
    WHERE replayed_at IS NULL;

CREATE INDEX IF NOT EXISTS outbox_dlq_org_id_idx ON outbox_dlq (org_id);

ALTER TABLE outbox_dlq ENABLE ROW LEVEL SECURITY;
ALTER TABLE outbox_dlq FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS outbox_dlq_tenant_isolation ON outbox_dlq;
CREATE POLICY outbox_dlq_tenant_isolation ON outbox_dlq
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

DROP POLICY IF EXISTS outbox_dlq_relay ON outbox_dlq;
CREATE POLICY outbox_dlq_relay ON outbox_dlq
    USING (current_setting('app.outbox_relay', true) = '1')
    WITH CHECK (current_setting('app.outbox_relay', true) = '1');
