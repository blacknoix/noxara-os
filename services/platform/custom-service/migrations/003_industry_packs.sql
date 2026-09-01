-- Phase 4.5 — Industry pack installs + optimistic concurrency on custom records.
-- PG16: no CREATE POLICY IF NOT EXISTS; tolerate duplicate_object (42710).
-- Do NOT DROP POLICY under FORCE ROW LEVEL SECURITY.

ALTER TABLE custom_record
    ADD COLUMN IF NOT EXISTS version INT NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS custom_industry_pack_install (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    pack_id TEXT NOT NULL,
    pack_version TEXT NOT NULL,
    package_name TEXT NOT NULL,
    package_install_id UUID,
    marketplace_connector_key TEXT,
    marketplace_install_public_id TEXT,
    seed_applied JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'installed'
        CHECK (status IN ('installed', 'uninstalled')),
    installed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    installed_by UUID NOT NULL,
    uninstalled_at TIMESTAMPTZ,
    uninstalled_by UUID,
    UNIQUE (org_id, public_id)
);

-- At most one active install per pack per org.
CREATE UNIQUE INDEX IF NOT EXISTS custom_industry_pack_install_active_uniq
    ON custom_industry_pack_install (org_id, pack_id)
    WHERE status = 'installed';

CREATE INDEX IF NOT EXISTS custom_industry_pack_install_org_idx
    ON custom_industry_pack_install (org_id, installed_at DESC);

ALTER TABLE custom_industry_pack_install ENABLE ROW LEVEL SECURITY;
ALTER TABLE custom_industry_pack_install FORCE ROW LEVEL SECURITY;

CREATE POLICY custom_industry_pack_install_tenant_isolation ON custom_industry_pack_install
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
