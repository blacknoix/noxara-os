# Sandbox organization

In-repo seedable sandbox (not a hosted multi-tenant developer portal).

```bash
# After docker/dev-up and migrations:
./scripts/seed-sandbox-dev.sh
# Writes .tmp/sandbox.env with:
#   SANDBOX_ORG_ID=org_…
#   SANDBOX_API_KEY=…
```

Use `SANDBOX_API_KEY` with the generated SDKs and the fixture third-party client
under `packages/sdk/fixtures/third_party_client.mjs`.

Default scopes: `sales.customer.read`, `sales.customer.create`,
`finance.invoice.read`, `finance.invoice.create`, `finance.invoice.issue`,
`admin.webhook.read`, `admin.webhook.write`.
