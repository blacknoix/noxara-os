# API

## Core (`companyos-core`)

- `GET /healthz`, `/livez`, `/readyz`
- `GET|POST /api/v1/hello` (tenant-scoped; LOCAL-ONLY auth)
- `GET /api/v1/dashboard` — dashboard BFF widget snapshot (CRM + Finance aggregates)
- `GET /api/v1/openapi.json` — core OpenAPI (merged export includes CRM + Finance)
- `GET /api/v1/workspace/...` — orgs, members, roles, teams, capabilities

## CRM (`companyos-crm`)

Sales bounded context, mounted at **`/api/v1/sales/...`** (proxied by the gateway).

- `GET /api/v1/sales/pipelines` — list pipelines
- `GET /api/v1/sales/pipelines/{id}/board` — kanban board
- `GET|POST /api/v1/sales/customers`, deals, leads, quotes, activities, products
- `GET /api/v1/sales/quotes/{id}/invoice-action` — whether Finance can create an invoice
- `GET /api/v1/sales/reports/summary` — pipeline by stage, win rate, forecast
- `GET /api/v1/sales/openapi.json` — CRM-only OpenAPI document

## Finance (`companyos-finance`)

Finance bounded context, mounted at **`/api/v1/finance/...`** (proxied by the gateway).

- `GET|POST /api/v1/finance/invoices` — drafts; `POST .../issue|send|void`
- `POST /api/v1/finance/invoices/from-quote` — quote snapshot → draft invoice
- `GET|POST /api/v1/finance/payments` — record + allocate
- `POST /api/v1/finance/credit-notes`
- `GET|POST /api/v1/finance/expenses` — submit / decide (approval_limit)
- `GET /api/v1/finance/reports/summary`
- `POST /api/v1/finance/webhooks/stripe` — idempotent provider fixtures
- `POST /api/v1/finance/events/sales/apply` — in-process CRM event projection (tests)
- `GET /api/v1/finance/openapi.json`

`Idempotency-Key` on POST issue/pay/credit. `If-Match` on draft invoice PATCH only.

Gateway URL: same host as core (`PUBLIC_API_URL`), path prefixes `/api/v1/sales` and `/api/v1/finance`.

Errors use RFC 9457 `application/problem+json` with stable `code` and `request_id`.
