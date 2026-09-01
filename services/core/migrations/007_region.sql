-- Phase 4.1: organization.region (ADR-015) — tenant home region, immutable after create.
-- Region is a routing/residency attribute, not an authz permission.

ALTER TABLE organization
    ADD COLUMN IF NOT EXISTS region TEXT NOT NULL DEFAULT 'us';

-- Backfill any NULL (defensive; DEFAULT covers new/existing rows).
UPDATE organization SET region = 'us' WHERE region IS NULL OR region = '';

DO $$
BEGIN
    ALTER TABLE organization
        ADD CONSTRAINT organization_region_check
        CHECK (region IN ('us', 'eu', 'ap'));
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS organization_region_idx ON organization (region);

-- Immutability: reject UPDATEs that change region (change-region is an explicit
-- Temporal migration workflow in a later phase; default is immutable).
CREATE OR REPLACE FUNCTION organization_region_immutable()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.region IS DISTINCT FROM OLD.region THEN
        RAISE EXCEPTION 'organization.region is immutable after creation (ADR-015 / Phase 4.1)'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS organization_region_immutable_trg ON organization;
CREATE TRIGGER organization_region_immutable_trg
    BEFORE UPDATE OF region ON organization
    FOR EACH ROW
    EXECUTE FUNCTION organization_region_immutable();

-- Control-plane audit for routing / failover / residency admin actions.
CREATE TABLE IF NOT EXISTS region_routing_audit (
    id UUID PRIMARY KEY,
    org_id UUID REFERENCES organization(id),
    actor_user_id UUID,
    action TEXT NOT NULL,
    detail JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS region_routing_audit_org_id_idx
    ON region_routing_audit (org_id);
CREATE INDEX IF NOT EXISTS region_routing_audit_created_at_idx
    ON region_routing_audit (created_at DESC);
