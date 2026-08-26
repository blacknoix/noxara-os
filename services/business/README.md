# Business service group (Phase 1+)

Sales CRM lives in **`crm-service/`** (Phase 1.4). Customer, lead, deal, pipeline, quote, product, activity, and import data are mastered in that bounded context (see ADR 009).

Finance lives in **`finance-service/`** (Phase 1.5). It projects customers from Sales events, owns invoices/payments/ledger, and never reads `sales_*` tables.

Projects & Tasks live in **`project-service/`** (Phase 1.6). Operations context owns `operations_*` tables, projects DealWon into projects, and never reads CRM or Finance tables.

## Network boundary

Even when co-located with core in development, business services are reached through the **API gateway** — not by direct in-process calls from other domains. Splitting to separate processes is configuration-only (same crates/binaries, different topology / transport).

## Layout

| Path | Phase | Description |
|------|-------|-------------|
| `crm-service/` | 1.4 | Sales CRM — customers, leads, deals, pipelines, quotes |
| `finance-service/` | 1.5 | Invoicing, payments, expenses, double-entry journal |
| `project-service/` | 1.6 | Projects, tasks, board, my-work, DealWon projection |
