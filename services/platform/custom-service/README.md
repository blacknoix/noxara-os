# companyos-custom — Phase 4.4 / 4.5 Low-code builder + industry packs

Tenant-scoped custom entities, records, views/layouts, formula fields, a
capped scripting sandbox, versioned customisation packages, and industry
vertical packs (configuration + marketplace listings — not domain forks).

## Routes

| Area | Path |
|------|------|
| Definitions | `/api/v1/custom/entities` |
| Records | `/api/v1/custom/records/{slug}` (PATCH requires `If-Match` version) |
| Views / layouts / scripts | `/api/v1/custom/views|layouts|scripts/{slug}` |
| Packages | `/api/v1/custom/packages/export\|import` |
| Industry packs | `/api/v1/custom/industry-packs` (+ `/{id}/install|uninstall`) |

## Industry packs (4.5)

Shipped packs (embedded JSON under `packs/`):

1. `professional-services` — engagements / retainers
2. `retail` — product SKU + POS-light session
3. `light-manufacturing` — BOM line + work order (not MRP)
4. `healthcare-admin` — appointments + admin notes (not EHR; no PHI authz bypass)

Install imports `companyos.custom.package`, applies OrgProvisioning-style seed
rows, and best-effort installs the matching marketplace listing
(`industry.*` connector keys). Uninstall does **not** delete tenant data.

Domain services under `services/business/**` must not branch on pack id —
enforced by `phase45_industry_packs` grep test.

## Authz

- Builder: `custom.builder.read` / `custom.builder.manage` (Member denied manage by default)
- Packages / pack install: `custom.package.export` / `custom.package.import`
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
