# Runbook: deal-to-cash walkthrough

Flagship product loop: **sign up → org → invite/role → customer/deal → quote → accept → invoice → payment**, with journal balance and tenant isolation.

Still the Phase 1–4 flagship path after industry modules (4.5): money stays integer minor units, issued invoices are immutable, and payment allocations never exceed invoice total.

Automated coverage: `cargo test -p companyos-finance --test deal_to_cash`.

## Prerequisites

```bash
cp .env.example .env
make dev-up          # compose + migrate + seed + core/crm/finance/.../gateway/ai
pnpm install
pnpm --filter @companyos/web dev   # http://127.0.0.1:3000
```

API base: `http://127.0.0.1:8080` (gateway). Mail links print to logs and `.tmp/mail/`.

`COMPANYOS_LOCAL_AUTH` defaults to **0**. Share `AUTH_JWT_SECRET` across services (dev-up does this).

## Seeded shortcut

`scripts/seed-dev.sh` (via `make dev-up`) creates **Acme Demo** with:

- Owner `owner@acme.demo` (MFA required after password)
- Member `member@acme.demo` / `correct-horse-battery` (no MFA)
- OrgProvisioning: system roles, pipeline seed stages, expense categories

Ledger accounts materialize on first finance write. Prefer the full register path below once, so you exercise verification + invite.

## Walkthrough (UI or API)

### 1. Sign up / verify

- UI: `/signup` → check `.tmp/mail/` or core logs for verify link → `/verify-email`
- API: `POST /api/v1/auth/register` then `POST /api/v1/auth/verify-email` with `{"token"}`

Register creates the org and runs **OrgProvisioning** (roles, numbering, pipeline seed stages).

### 2. Org + invite / role

- Login as Owner (complete MFA enrollment on `/mfa` if prompted).
- Settings → invite a Sales user: `POST /api/v1/workspace/members/invite` `{"email","role":"sales"}`
- Accept via mail link `/invite/accept?token=…` or `POST /api/v1/workspace/invitations/accept`

Sales can create customers/deals/quotes; cannot issue invoices.

### 3. Customer + deal → Won

- `POST /api/v1/sales/customers` `{ "name", "email?" }`
- `POST /api/v1/sales/deals` `{ "name", "amount_minor", "customer_id" }` (default pipeline)
- `POST /api/v1/sales/deals/{id}/win` `{ "reason" }` — **idempotent** (one DealWon event)

### 4. Quote → accept (immutable)

- Optional catalogue: `POST /api/v1/sales/products`
- `POST /api/v1/sales/quotes` with `deal_id`, `customer_id`, `lines[]`
- `POST /api/v1/sales/quotes/{id}/accept`
- Editing an accepted quote **forks** a new draft (`201`); original stays `accepted`

### 5–7. Invoice from quote → issue → pay

Finance never reads CRM tables. Build a snapshot from the accepted quote:

```http
POST /api/v1/finance/invoices/from-quote
{
  "quote_id": "qte_…",
  "customer_id": "cus_…",
  "customer_name": "…",
  "currency": "USD",
  "lines": [{ "description", "quantity", "unit_price_minor", "discount_minor", "tax_rate_bps" }]
}
```

- `POST /api/v1/finance/invoices/{id}/issue` → gapless `INV-{year}-{NNNNNN}`, journal posts, immutable
- `POST /api/v1/finance/payments` with `invoice_id` + `amount_minor` (allocates in one shot)

Invoice should reach `status=paid`, `balance_minor=0`.

### 8. Assert

| Check | Expectation |
|---|---|
| Journal | Every entry: Σ debit = Σ credit |
| Balance identity | `balance = total - amount_paid - amount_credited` |
| Customer continuity | Same `cus_…` on customer, quote, invoice, payment |
| Authz | Sales cannot issue invoices; Owner/Finance can |
| Tenant isolation | Org B token → `404` on Org A invoice/customer |

## Manual service commands (without full `dev-up`)

If compose deps are already up:

```bash
export $(grep -v '^#' .env | xargs)
cargo run -p companyos-core
cargo run -p companyos-crm
cargo run -p companyos-finance
cargo run -p companyos-project
cargo run -p companyos-ai          # mock provider unless AI_API_KEY set
cargo run -p companyos-gateway
# workers (optional for deal-to-cash):
cargo run -p companyos-outbox-relay
cargo run -p companyos-project-worker
cargo run -p companyos-notification
```

OpenSearch / ClickHouse are **not** required for this path:

```bash
docker compose -f infrastructure/docker/docker-compose.yml --profile full up -d
```
