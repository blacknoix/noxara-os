# Versioning

- URL major version: `/api/v1/...`
- **Additive changes only** within a major version
- Breaking changes require `/api/v2` (future)
- Public contract: `packages/sdk/openapi.public.json` and `GET /api/v1/openapi.public.json`

See [deprecation.md](./deprecation.md) for the 180-day dual-publish window.
