# CompanyOS Public API — Developer Guide

Authenticate with an **organization API key**. Keys are created by an Owner/Admin in
Settings → Security (or `POST /api/v1/governance/api-keys`) and shown **once**.

```http
Authorization: Bearer cos_…   # or
X-Api-Key: cos_…
```

Scopes on the key are **permission IDs** (e.g. `sales.customer.read`). At exchange
time they are intersected with the key owner's role permissions — the **narrower**
of the two wins. A Member cannot mint unrestricted keys; only holders of
`admin.api_key.manage` can create keys.

See [scopes.md](./scopes.md), [versioning.md](./versioning.md), [webhooks.md](./webhooks.md),
[deprecation.md](./deprecation.md), and the [quickstart](./quickstart.md).

Live third-party developer validation against a hosted sandbox is **out of this PR**;
use the in-repo sandbox seed + fixture client instead.
