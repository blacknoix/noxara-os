# API

## Core (`companyos-core`)

- `GET /healthz`, `/livez`, `/readyz`
- `GET|POST /api/v1/hello` (tenant-scoped; LOCAL-ONLY auth)
- `GET /api/v1/dashboard` — dashboard BFF widget snapshot
- `GET /api/v1/openapi.json` — core OpenAPI (merged export includes CRM)
- `GET /api/v1/workspace/...` — orgs, members, roles, teams, capabilities

## CRM (`companyos-crm`)

Sales bounded context, mounted at **`/api/v1/sales/...`** (proxied by the gateway).

- `GET /api/v1/sales/pipelines` — list pipelines
- `GET /api/v1/sales/pipelines/{id}/board` — kanban board
- `GET|POST /api/v1/sales/customers`, deals, leads, quotes, activities, products
- `GET /api/v1/sales/reports/summary` — pipeline by stage, win rate, forecast
- `GET /api/v1/sales/openapi.json` — CRM-only OpenAPI document

Gateway URL: same host as core (`PUBLIC_API_URL`), path prefix `/api/v1/sales`.

Errors use RFC 9457 `application/problem+json` with stable `code` and `request_id`.
