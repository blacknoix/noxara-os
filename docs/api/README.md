# API

Phase 0 surface:

- `GET /healthz`, `/livez`, `/readyz`
- `GET|POST /api/v1/hello` (tenant-scoped; LOCAL-ONLY auth)
- `GET /api/v1/openapi.json`

Errors use RFC 9457 `application/problem+json` with stable `code` and `request_id`.
