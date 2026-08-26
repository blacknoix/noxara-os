-- ClickHouse fact table for invoice issued events (ADR-011).
-- Applied only when CLICKHOUSE_URL is set (see analytics-service ingest).
-- Never dual-written from OLTP app transactions.

CREATE TABLE IF NOT EXISTS fact_invoice_issued (
    org_id UUID,
    invoice_id String,
    event_id UUID,
    occurred_at DateTime64(3, 'UTC'),
    amount_minor Int64,
    currency LowCardinality(String),
    ingested_at DateTime64(3, 'UTC') DEFAULT now64(3)
) ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (org_id, invoice_id, event_id);
