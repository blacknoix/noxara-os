-- Phase 1.8 search indexer job tracking.

CREATE TABLE IF NOT EXISTS search_index_job (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    requested_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS search_index_job_org_idx ON search_index_job (org_id, created_at DESC);

ALTER TABLE search_index_job ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_index_job FORCE ROW LEVEL SECURITY;

CREATE POLICY search_index_job_tenant_isolation ON search_index_job
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE POLICY search_index_job_ingest ON search_index_job
    USING (current_setting('app.search_ingest', true) = '1')
    WITH CHECK (current_setting('app.search_ingest', true) = '1');
