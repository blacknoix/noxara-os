-- Phase 4.4 low-code builder — custom entities, records, views, scripts, packages.
-- PG16: no CREATE POLICY IF NOT EXISTS; tolerate duplicate_object (42710).
-- Do NOT DROP POLICY under FORCE ROW LEVEL SECURITY.

CREATE TABLE IF NOT EXISTS custom_entity_definition (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    slug TEXT NOT NULL,
    label TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    fields JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'draft',
    published_version INT NOT NULL DEFAULT 0,
    created_by UUID NOT NULL,
    updated_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, slug)
);

CREATE INDEX IF NOT EXISTS custom_entity_definition_org_idx
    ON custom_entity_definition (org_id, updated_at DESC);

ALTER TABLE custom_entity_definition ENABLE ROW LEVEL SECURITY;
ALTER TABLE custom_entity_definition FORCE ROW LEVEL SECURITY;

CREATE POLICY custom_entity_definition_tenant_isolation ON custom_entity_definition
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Records: open-ended field values in JSONB with documented schema per definition.
CREATE TABLE IF NOT EXISTS custom_record (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    entity_id UUID NOT NULL REFERENCES custom_entity_definition (id),
    entity_slug TEXT NOT NULL,
    values JSONB NOT NULL DEFAULT '{}'::jsonb,
    search_text TEXT NOT NULL DEFAULT '',
    created_by UUID NOT NULL,
    updated_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    UNIQUE (org_id, public_id)
);

CREATE INDEX IF NOT EXISTS custom_record_org_entity_idx
    ON custom_record (org_id, entity_slug, updated_at DESC);
CREATE INDEX IF NOT EXISTS custom_record_search_idx
    ON custom_record (org_id, entity_slug, search_text);

ALTER TABLE custom_record ENABLE ROW LEVEL SECURITY;
ALTER TABLE custom_record FORCE ROW LEVEL SECURITY;

CREATE POLICY custom_record_tenant_isolation ON custom_record
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS custom_view (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    entity_slug TEXT NOT NULL,
    name TEXT NOT NULL,
    columns JSONB NOT NULL DEFAULT '[]'::jsonb,
    filters JSONB NOT NULL DEFAULT '[]'::jsonb,
    sort JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_by UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);

CREATE INDEX IF NOT EXISTS custom_view_org_slug_idx
    ON custom_view (org_id, entity_slug);

ALTER TABLE custom_view ENABLE ROW LEVEL SECURITY;
ALTER TABLE custom_view FORCE ROW LEVEL SECURITY;

CREATE POLICY custom_view_tenant_isolation ON custom_view
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS custom_layout (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    entity_slug TEXT NOT NULL,
    name TEXT NOT NULL,
    sections JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_by UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, entity_slug)
);

ALTER TABLE custom_layout ENABLE ROW LEVEL SECURITY;
ALTER TABLE custom_layout FORCE ROW LEVEL SECURITY;

CREATE POLICY custom_layout_tenant_isolation ON custom_layout
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS custom_script (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    entity_slug TEXT NOT NULL,
    hook TEXT NOT NULL,
    source TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_by UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, entity_slug, hook)
);

ALTER TABLE custom_script ENABLE ROW LEVEL SECURITY;
ALTER TABLE custom_script FORCE ROW LEVEL SECURITY;

CREATE POLICY custom_script_tenant_isolation ON custom_script
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS custom_package_install (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    package_name TEXT NOT NULL,
    package_version TEXT NOT NULL,
    artifact JSONB NOT NULL,
    installed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    installed_by UUID NOT NULL,
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, package_name, package_version)
);

ALTER TABLE custom_package_install ENABLE ROW LEVEL SECURITY;
ALTER TABLE custom_package_install FORCE ROW LEVEL SECURITY;

CREATE POLICY custom_package_install_tenant_isolation ON custom_package_install
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Platform schema marker used by upgrade-rehearsal CI (additive bump).
CREATE TABLE IF NOT EXISTS custom_platform_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO custom_platform_meta (key, value)
VALUES ('schema_version', '1')
ON CONFLICT (key) DO NOTHING;
