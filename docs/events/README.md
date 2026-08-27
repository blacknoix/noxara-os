# Events

Subject format: `companyos.{org_id}.{context}.{aggregate}.{event}.v{n}`

## Schema registry

JSON Schema contracts live under [`schemas/`](./schemas/). Each file is named
`{context}.{aggregate}.{event}.v{n}.json` and validates the wire
[`EventEnvelope`](../../crates/events/src/lib.rs) (required envelope fields +
payload keys).

Contract tests in `crates/events` (`contracts` module / `tests/contract_schemas.rs`)
load every schema, build a sample envelope via `EventEnvelope::new`, and assert
required fields match. Run:

```bash
cargo test -p companyos-events
```

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

## Operations / Approvals (Phase 1.7)

- `companyos.{org_}.operations.approval.requested.v1`
- `companyos.{org_}.operations.approval.decided.v1`

Emitted in the same transaction as the approval write. Finance/CRM call the Operations approval API (no cross-context table reads); Temporal activities call service APIs with `on_behalf_of` recorded on decisions.

## People / HR (Phase 2.1)

- `companyos.{org_}.people.employee.created.v1`
- `companyos.{org_}.people.employee.updated.v1`
- `companyos.{org_}.people.employee.onboarded.v1`
- `companyos.{org_}.people.employee.offboarded.v1`

Restricted fields (compensation, government IDs, bank/tax) are never included in event payloads. Departments remain Workspace-owned (`dep_`); HR stores opaque department ids (ADR 020).

## Platform consumers (Phase 1.8)

- Outbox → NATS via `companyos-outbox-relay` (`scripts/nats-bootstrap.sh` creates
  `COMPANYOS_EVENTS` + `COMPANYOS_EVENTS_DLQ` + durable `platform-consumers`)
- Notification fan-out, search indexer, and analytics facts ingest envelopes
  with consumer-side idempotency on `idempotency_key`
- Lag / DLQ runbook: [`docs/runbooks/outbox-lag.md`](../runbooks/outbox-lag.md)
