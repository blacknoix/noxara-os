-- Phase 1.8 analytics (ADR-011: warehouse facts fed ONLY from events).
-- Postgres mirror of fact_invoice_issued for CI when CLICKHOUSE_URL is unset.

CREATE TABLE IF NOT EXISTS analytics_cursor (
    consumer_name TEXT PRIMARY KEY,
    last_event_id UUID,
    last_occurred_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS analytics_fact_invoice_issued (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    invoice_id TEXT NOT NULL,
    amount_minor BIGINT,
    currency TEXT,
    issued_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS analytics_fact_invoice_issued_org_idx
    ON analytics_fact_invoice_issued (org_id, issued_at DESC);

ALTER TABLE analytics_fact_invoice_issued ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_invoice_issued FORCE ROW LEVEL SECURITY;

CREATE POLICY IF NOT EXISTS analytics_fact_invoice_issued_tenant ON analytics_fact_invoice_issued
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE POLICY IF NOT EXISTS analytics_fact_invoice_issued_ingest ON analytics_fact_invoice_issued
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');
