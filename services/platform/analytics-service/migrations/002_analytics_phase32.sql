-- Phase 3.2 analytics & reporting.
-- Facts remain event-derived (ADR-011). Postgres is the CI / default store;
-- ClickHouse DDL lives under clickhouse/ and is preferred when CLICKHOUSE_URL is set.

-- ---------------------------------------------------------------------------
-- Raw event log (idempotent ingest mirror)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS analytics_events_raw (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    subject TEXT NOT NULL,
    context TEXT NOT NULL,
    aggregate TEXT NOT NULL,
    event_type TEXT NOT NULL,
    version INT NOT NULL DEFAULT 1,
    occurred_at TIMESTAMPTZ NOT NULL,
    actor_kind TEXT,
    actor_user_id UUID,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS analytics_events_raw_org_idx
    ON analytics_events_raw (org_id, occurred_at DESC);

ALTER TABLE analytics_events_raw ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_events_raw FORCE ROW LEVEL SECURITY;

CREATE POLICY analytics_events_raw_tenant ON analytics_events_raw
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE POLICY analytics_events_raw_ingest ON analytics_events_raw
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

-- ---------------------------------------------------------------------------
-- Typed fact tables (org_id leading key)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS analytics_fact_deal_stage_change (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    deal_id TEXT NOT NULL,
    from_stage TEXT,
    to_stage TEXT,
    amount_minor BIGINT,
    currency TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS analytics_fact_deal_stage_change_org_idx
    ON analytics_fact_deal_stage_change (org_id, occurred_at DESC);
ALTER TABLE analytics_fact_deal_stage_change ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_deal_stage_change FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_fact_deal_stage_change_tenant ON analytics_fact_deal_stage_change
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_fact_deal_stage_change_ingest ON analytics_fact_deal_stage_change
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

CREATE TABLE IF NOT EXISTS analytics_fact_invoice_lifecycle (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    invoice_id TEXT NOT NULL,
    lifecycle_event TEXT NOT NULL,
    amount_minor BIGINT,
    currency TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS analytics_fact_invoice_lifecycle_org_idx
    ON analytics_fact_invoice_lifecycle (org_id, occurred_at DESC);
ALTER TABLE analytics_fact_invoice_lifecycle ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_invoice_lifecycle FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_fact_invoice_lifecycle_tenant ON analytics_fact_invoice_lifecycle
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_fact_invoice_lifecycle_ingest ON analytics_fact_invoice_lifecycle
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

CREATE TABLE IF NOT EXISTS analytics_fact_payment (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    payment_id TEXT NOT NULL,
    invoice_id TEXT,
    amount_minor BIGINT,
    currency TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS analytics_fact_payment_org_idx
    ON analytics_fact_payment (org_id, occurred_at DESC);
ALTER TABLE analytics_fact_payment ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_payment FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_fact_payment_tenant ON analytics_fact_payment
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_fact_payment_ingest ON analytics_fact_payment
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

CREATE TABLE IF NOT EXISTS analytics_fact_expense (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    expense_id TEXT NOT NULL,
    lifecycle_event TEXT NOT NULL,
    amount_minor BIGINT,
    currency TEXT,
    category TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS analytics_fact_expense_org_idx
    ON analytics_fact_expense (org_id, occurred_at DESC);
ALTER TABLE analytics_fact_expense ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_expense FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_fact_expense_tenant ON analytics_fact_expense
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_fact_expense_ingest ON analytics_fact_expense
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

CREATE TABLE IF NOT EXISTS analytics_fact_task_lifecycle (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    task_id TEXT NOT NULL,
    lifecycle_event TEXT NOT NULL,
    project_id TEXT,
    status TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS analytics_fact_task_lifecycle_org_idx
    ON analytics_fact_task_lifecycle (org_id, occurred_at DESC);
ALTER TABLE analytics_fact_task_lifecycle ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_task_lifecycle FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_fact_task_lifecycle_tenant ON analytics_fact_task_lifecycle
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_fact_task_lifecycle_ingest ON analytics_fact_task_lifecycle
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

CREATE TABLE IF NOT EXISTS analytics_fact_ai_usage (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    usage_kind TEXT NOT NULL,
    tokens BIGINT,
    model TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS analytics_fact_ai_usage_org_idx
    ON analytics_fact_ai_usage (org_id, occurred_at DESC);
ALTER TABLE analytics_fact_ai_usage ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_ai_usage FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_fact_ai_usage_tenant ON analytics_fact_ai_usage
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_fact_ai_usage_ingest ON analytics_fact_ai_usage
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

CREATE TABLE IF NOT EXISTS analytics_fact_api_request (
    event_id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    route TEXT,
    method TEXT,
    status_code INT,
    duration_ms BIGINT,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS analytics_fact_api_request_org_idx
    ON analytics_fact_api_request (org_id, occurred_at DESC);
ALTER TABLE analytics_fact_api_request ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_fact_api_request FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_fact_api_request_tenant ON analytics_fact_api_request
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_fact_api_request_ingest ON analytics_fact_api_request
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

-- ---------------------------------------------------------------------------
-- Daily / weekly rollups for dashboard widgets
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS analytics_rollup_daily (
    org_id UUID NOT NULL,
    metric_name TEXT NOT NULL,
    day DATE NOT NULL,
    value_minor BIGINT NOT NULL DEFAULT 0,
    value_count BIGINT NOT NULL DEFAULT 0,
    currency TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, metric_name, day)
);
ALTER TABLE analytics_rollup_daily ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_rollup_daily FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_rollup_daily_tenant ON analytics_rollup_daily
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_rollup_daily_ingest ON analytics_rollup_daily
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

CREATE TABLE IF NOT EXISTS analytics_rollup_weekly (
    org_id UUID NOT NULL,
    metric_name TEXT NOT NULL,
    week_start DATE NOT NULL,
    value_minor BIGINT NOT NULL DEFAULT 0,
    value_count BIGINT NOT NULL DEFAULT 0,
    currency TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, metric_name, week_start)
);
ALTER TABLE analytics_rollup_weekly ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_rollup_weekly FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_rollup_weekly_tenant ON analytics_rollup_weekly
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_rollup_weekly_ingest ON analytics_rollup_weekly
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

-- ---------------------------------------------------------------------------
-- Metadata freshness (eventual consistency labelling)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS analytics_freshness (
    org_id UUID PRIMARY KEY,
    last_event_at TIMESTAMPTZ,
    last_ingest_at TIMESTAMPTZ,
    lag_seconds BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE analytics_freshness ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_freshness FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_freshness_tenant ON analytics_freshness
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
CREATE POLICY analytics_freshness_ingest ON analytics_freshness
    USING (current_setting('app.analytics_ingest', true) = '1')
    WITH CHECK (current_setting('app.analytics_ingest', true) = '1');

-- ---------------------------------------------------------------------------
-- Saved reports, dashboards, schedules, runs (org-scoped config)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS analytics_report (
    id UUID PRIMARY KEY,
    public_id TEXT NOT NULL,
    org_id UUID NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    definition JSONB NOT NULL,
    visualization TEXT NOT NULL DEFAULT 'table',
    created_by UUID NOT NULL,
    updated_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS analytics_report_org_idx ON analytics_report (org_id, updated_at DESC);
ALTER TABLE analytics_report ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_report FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_report_tenant ON analytics_report
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS analytics_dashboard (
    id UUID PRIMARY KEY,
    public_id TEXT NOT NULL,
    org_id UUID NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    layout JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_by UUID NOT NULL,
    updated_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS analytics_dashboard_org_idx ON analytics_dashboard (org_id, updated_at DESC);
ALTER TABLE analytics_dashboard ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_dashboard FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_dashboard_tenant ON analytics_dashboard
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS analytics_dashboard_widget (
    id UUID PRIMARY KEY,
    public_id TEXT NOT NULL,
    org_id UUID NOT NULL,
    dashboard_id UUID NOT NULL REFERENCES analytics_dashboard(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    visualization TEXT NOT NULL DEFAULT 'stat',
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    position INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS analytics_dashboard_widget_dash_idx
    ON analytics_dashboard_widget (org_id, dashboard_id, position);
ALTER TABLE analytics_dashboard_widget ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_dashboard_widget FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_dashboard_widget_tenant ON analytics_dashboard_widget
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS analytics_schedule (
    id UUID PRIMARY KEY,
    public_id TEXT NOT NULL,
    org_id UUID NOT NULL,
    report_id UUID NOT NULL REFERENCES analytics_report(id) ON DELETE CASCADE,
    cron TEXT NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    channel TEXT NOT NULL DEFAULT 'notification',
    recipients JSONB NOT NULL DEFAULT '[]'::jsonb,
    export_format TEXT NOT NULL DEFAULT 'csv',
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_by UUID NOT NULL,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS analytics_schedule_org_idx ON analytics_schedule (org_id, enabled);
ALTER TABLE analytics_schedule ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_schedule FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_schedule_tenant ON analytics_schedule
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS analytics_run (
    id UUID PRIMARY KEY,
    public_id TEXT NOT NULL,
    org_id UUID NOT NULL,
    report_id UUID,
    schedule_id UUID,
    kind TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    started_by UUID,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    row_count INT,
    file_id TEXT,
    error TEXT,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS analytics_run_org_idx ON analytics_run (org_id, started_at DESC);
ALTER TABLE analytics_run ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics_run FORCE ROW LEVEL SECURITY;
CREATE POLICY analytics_run_tenant ON analytics_run
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
