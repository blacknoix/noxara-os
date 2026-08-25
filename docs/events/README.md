# Events

Subject format: `companyos.{org_id}.{context}.{aggregate}.{event}.v{n}`

## Core (Phase 0+)

- `companyos.{org_}.core.hello.created.v1` via the transactional outbox

## Sales / CRM (Phase 1.4)

CRM emits sales-domain events through the same outbox pattern (examples):

- `companyos.{org_}.sales.customer.created.v1`
- `companyos.{org_}.sales.deal.created.v1`
- `companyos.{org_}.sales.deal.won.v1`
- `companyos.{org_}.sales.quote.sent.v1`

Exact catalogue lives in `services/business/crm-service` event definitions.
