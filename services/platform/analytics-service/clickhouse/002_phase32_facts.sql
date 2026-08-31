-- Phase 3.2 event-derived analytics facts. Operational services never write these.

CREATE TABLE IF NOT EXISTS fact_invoice_lifecycle (
    org_id UUID,
    invoice_id String,
    lifecycle_event LowCardinality(String),
    event_id UUID,
    amount_minor Int64,
    currency LowCardinality(String),
    occurred_at DateTime64(3, 'UTC'),
    ingested_at DateTime64(3, 'UTC') DEFAULT now64(3)
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (org_id, invoice_id, occurred_at, event_id);

CREATE TABLE IF NOT EXISTS fact_payment (
    org_id UUID,
    payment_id String,
    invoice_id String,
    event_id UUID,
    amount_minor Int64,
    currency LowCardinality(String),
    occurred_at DateTime64(3, 'UTC'),
    ingested_at DateTime64(3, 'UTC') DEFAULT now64(3)
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (org_id, payment_id, occurred_at, event_id);

CREATE TABLE IF NOT EXISTS fact_expense (
    org_id UUID,
    expense_id String,
    lifecycle_event LowCardinality(String),
    event_id UUID,
    amount_minor Int64,
    currency LowCardinality(String),
    category String,
    occurred_at DateTime64(3, 'UTC'),
    ingested_at DateTime64(3, 'UTC') DEFAULT now64(3)
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (org_id, expense_id, occurred_at, event_id);

CREATE TABLE IF NOT EXISTS fact_deal_stage_change (
    org_id UUID,
    deal_id String,
    from_stage String,
    to_stage String,
    event_id UUID,
    amount_minor Int64,
    currency LowCardinality(String),
    occurred_at DateTime64(3, 'UTC'),
    ingested_at DateTime64(3, 'UTC') DEFAULT now64(3)
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (org_id, deal_id, occurred_at, event_id);

CREATE TABLE IF NOT EXISTS fact_task_lifecycle (
    org_id UUID,
    task_id String,
    lifecycle_event LowCardinality(String),
    event_id UUID,
    project_id String,
    status LowCardinality(String),
    occurred_at DateTime64(3, 'UTC'),
    ingested_at DateTime64(3, 'UTC') DEFAULT now64(3)
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (org_id, task_id, occurred_at, event_id);

CREATE TABLE IF NOT EXISTS fact_ai_usage (
    org_id UUID,
    usage_kind LowCardinality(String),
    event_id UUID,
    tokens Int64,
    model LowCardinality(String),
    occurred_at DateTime64(3, 'UTC'),
    ingested_at DateTime64(3, 'UTC') DEFAULT now64(3)
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (org_id, usage_kind, occurred_at, event_id);

CREATE TABLE IF NOT EXISTS fact_api_request (
    org_id UUID,
    route String,
    method LowCardinality(String),
    status_code Int32,
    duration_ms Int64,
    event_id UUID,
    occurred_at DateTime64(3, 'UTC'),
    ingested_at DateTime64(3, 'UTC') DEFAULT now64(3)
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (org_id, route, occurred_at, event_id);
