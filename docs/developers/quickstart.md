# Quickstart — create a customer

Requires a sandbox org API key (see [sandbox.md](./sandbox.md)).

```bash
export COMPANYOS_API_KEY=…   # from sandbox seed
export COMPANYOS_API_URL=http://127.0.0.1:8080

curl -sS -X POST "$COMPANYOS_API_URL/api/v1/sales/customers" \
  -H "Authorization: Bearer $COMPANYOS_API_KEY" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: qs-$(date +%s)" \
  -d '{"name":"Acme Robotics","email":"billing@acme.example"}'
```

Issue an invoice (after creating a draft via Finance public routes):

```bash
curl -sS -X POST "$COMPANYOS_API_URL/api/v1/finance/invoices/{inv_…}/issue" \
  -H "Authorization: Bearer $COMPANYOS_API_KEY" \
  -H "Idempotency-Key: issue-$(date +%s)"
```

TypeScript (generated SDK) and Python (`@companyos` / `companyos_public`) clients
read the same public OpenAPI — see `packages/sdk` and `packages/sdk-python`.
