# Business service group (Phase 1+)

Sales CRM lives in **`crm-service/`** (Phase 1.4). Customer, lead, deal, pipeline, quote, product, activity, and import data are mastered in that bounded context (see ADR 009).

Finance, Operations, and People bounded contexts will follow in later phases. Finance (invoicing, payments) is intentionally separate from CRM.

## Network boundary

Even when co-located with core in development, business services are reached through the **API gateway** — not by direct in-process calls from other domains. Splitting to separate processes is configuration-only (same crates/binaries, different topology / transport).

## Layout

| Path | Phase | Description |
|------|-------|-------------|
| `crm-service/` | 1.4 | Sales CRM — customers, leads, deals, pipelines, quotes |
| *(future)* `finance-service/` | 1.5+ | Invoicing, payments, ledger projections |
