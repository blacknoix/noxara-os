-- Phase 3.5 — time entry fact stub for governed metrics catalogue.
-- Fed only from Operations timesheet/time_entry events via /internal/ingest.

CREATE TABLE IF NOT EXISTS analytics_fact_time_entry (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    time_entry_id TEXT NOT NULL,
    timesheet_id TEXT,
    project_id TEXT,
    membership_user_id TEXT,
    minutes BIGINT NOT NULL DEFAULT 0,
    billable_minutes BIGINT NOT NULL DEFAULT 0,
    lifecycle_event TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS analytics_fact_time_entry_org_idx
    ON analytics_fact_time_entry (org_id, occurred_at DESC);
ALTER TABLE analytics_fact_time_entry ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_time_entry FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_fact_time_entry_tenant ON analytics_fact_time_entry
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_fact_time_entry_ingest ON analytics_fact_time_entry
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');
