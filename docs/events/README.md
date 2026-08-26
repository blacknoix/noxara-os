# Events

Subject format: `companyos.{org_id}.{context}.{aggregate}.{event}.v{n}`

## Core (Phase 0+)

- `companyos.{org_}.core.hello.created.v1` via the transactional outbox

## Sales / CRM (Phase 1.4)

CRM emits sales-domain events through the same outbox pattern (examples):

- `companyos.{org_}.sales.customer.created.v1`
- `companyos.{org_}.sales.deal.created.v1`
- `companyos.{org_}.sales.deal.won.v1`
- `companyos.{org_}.sales.quote.accepted.v1`

Exact catalogue lives in `services/business/crm-service` event definitions.

Finance **projects** `sales.customer.created` (and ignores deal/quote for projection); quote→invoice uses an API snapshot, not CRM table reads.

## Finance (Phase 1.5)

- `companyos.{org_}.finance.invoice.issued.v1`
- `companyos.{org_}.finance.invoice.paid.v1`
- `companyos.{org_}.finance.payment.allocated.v1`
- `companyos.{org_}.finance.credit_note.issued.v1`
- `companyos.{org_}.finance.expense.submitted.v1` / `.approved.v1`

Emitted in the same transaction as the domain write via the shared outbox.

## Operations (Phase 1.6)

- `companyos.{org_}.operations.project.created.v1`
- `companyos.{org_}.operations.task.created.v1`
- `companyos.{org_}.operations.task.assigned.v1`
- `companyos.{org_}.operations.task.completed.v1`

Operations **projects** `sales.deal.won` via `POST /api/v1/operations/events/sales/apply` (opaque deal/customer ids only — never reads `sales_*` tables). Mentions write `operations_notification_intent` rows for authz-allowed recipients only.
