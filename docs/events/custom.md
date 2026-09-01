# Custom / low-code (Phase 4.4)

- `companyos.{org_}.custom.{entity_slug}.created.v1`
- `companyos.{org_}.custom.{entity_slug}.updated.v1`
- `companyos.{org_}.custom.{entity_slug}.deleted.v1`

Emitted in the same transaction as the record write via the shared outbox.
Fixture schema: `custom.demo_asset.created.v1` / `.updated.v1`.
Search document path uses `doc_type=custom:{slug}` and permission `custom.{slug}.read`.
