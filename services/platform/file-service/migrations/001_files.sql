-- Phase 1.8 file object metadata (blobs in MinIO / local stub).

CREATE TABLE IF NOT EXISTS file_object (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL UNIQUE,
    bucket TEXT NOT NULL,
    object_key TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    status TEXT NOT NULL DEFAULT 'pending'
);

CREATE INDEX IF NOT EXISTS file_object_org_idx ON file_object (org_id, created_at DESC);

ALTER TABLE file_object ENABLE ROW LEVEL SECURITY;
ALTER TABLE file_object FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS file_object_tenant_isolation ON file_object;
CREATE POLICY file_object_tenant_isolation ON file_object
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
