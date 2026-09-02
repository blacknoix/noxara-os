-- Phase ops: durable search doc mirror for OpenSearch degradation fallback.
CREATE TABLE IF NOT EXISTS search_doc_mirror (
    org_id UUID NOT NULL,
    doc_id TEXT NOT NULL,
    doc_type TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    href TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, doc_id)
);

CREATE INDEX IF NOT EXISTS search_doc_mirror_org_updated_idx
    ON search_doc_mirror (org_id, updated_at DESC);

ALTER TABLE search_doc_mirror ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_doc_mirror FORCE ROW LEVEL SECURITY;

CREATE POLICY search_doc_mirror_tenant_isolation ON search_doc_mirror
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE POLICY search_doc_mirror_ingest ON search_doc_mirror
    FOR ALL
    USING (current_setting('app.search_ingest', true) = '1')
    WITH CHECK (current_setting('app.search_ingest', true) = '1');
