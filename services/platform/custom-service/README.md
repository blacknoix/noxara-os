# companyos-custom — Phase 4.4 Low-code builder

Tenant-scoped custom entities, records, views/layouts, formula fields, a
capped scripting sandbox, and versioned customisation packages.

## Routes

| Area | Path |
|------|------|
| Definitions | `/api/v1/custom/entities` |
| Records | `/api/v1/custom/records/{slug}` |
| Views / layouts / scripts | `/api/v1/custom/views|layouts|scripts/{slug}` |
| Packages | `/api/v1/custom/packages/export\|import` |

## Authz

- Builder: `custom.builder.read` / `custom.builder.manage` (Member denied manage by default)
- Packages: `custom.package.export` / `custom.package.import`
- Per entity (registered on publish): `custom.{slug}.read` / `custom.{slug}.write`
- Fixture slug for deny-matrix: `custom.demo_asset.*`

## Sandbox

Purpose-built JSON AST interpreter (`src/sandbox`) — **not** host-language `eval`.
Hard caps on ops, approximate memory, and wall-clock time; network/disk/env host
functions denied. Fail closed on limit breach.

## Packaging

Format `companyos.custom.package` v1 JSON: entities, views, layouts, scripts,
permission names. Import is additive and tenant-scoped. Upgrade rehearsal applies
`migrations/002_platform_bump.sql` then re-runs CRUD.

## Bind

`CUSTOM_BIND` default `0.0.0.0:8096` (gateway `CUSTOM_SERVICE_URL`).
