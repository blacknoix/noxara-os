# 06-IMPLEMENTATION-PLAN

Status: **Active** outline. Phase 0–2.4 merged; Phase 2.5 is this slice.

## Completed

| Phase | Scope |
|-------|--------|
| 0 | Monorepo, hello slice, RLS, outbox, authz PDP, gateway stub, web shell stub |
| 1.1 | Identity & authentication (JWT, refresh, MFA, sessions, switch-org) |
| 1.2 | Workspace (orgs, members, roles, permissions, teams, invitations) |
| 1.3 | Application shell, design system, dashboard BFF, members saved views, axe CI |
| 1.4 | CRM / Sales (customers, pipeline, deals, quotes, import) |
| 1.5 | Finance v1 (invoices, payments, journal, expenses, quote→invoice) |
| 1.6 | Projects & Tasks / Operations (`companyos-project`) |
| 1.7 | Approval engine (operations / Temporal) |
| 1.8 | Platform events, notifications, search, analytics, files, outbox relay |
| 1.9 | AI Assistant MVP (copilot, proposals, retrieval) |
| 2.1 | People / HR v1 (employees, onboarding/offboarding) |
| 2.2 | Attendance & Leave (People / hr-service) |
| 2.3 | Payroll basics (`companyos-hr`): draft → calculate → approve → paid, journals via Finance HTTP, Temporal `PayrollRun` (ADR 021) |
| 2.4 | Finance CoA, periods, bank rec, expense policy depth (ADR 022) |

## Phase 2.5 — Inventory & Procurement (`companyos-inventory`) (this slice)

- New standalone service `companyos-inventory` (port 8093), API `/api/v1/inventory/...`,
  events under `Context::Inventory`. Shares core Postgres; never touches `people_*` or
  `finance_*` tables directly (journals + vendor bills go through Finance HTTP).
- Authz: `inventory.item.read|write`, `inventory.warehouse.read|write`,
  `inventory.stock.read`, `inventory.stock.move`, `inventory.supplier.read|write`,
  `inventory.purchase_request.read|write`, `inventory.purchase_order.read|write`,
  `inventory.goods_receipt.read|write`, `inventory.asset.read|write`
- Schema (`inventory_*`, all RLS): `warehouse`, `item`, `stock_level` (cache) +
  `stock_movement` (append-only source of truth), `supplier`, `purchase_request(_line)`,
  `purchase_order(_line)`, `goods_receipt(_line)`, `asset`, `asset_assignment`,
  `maintenance_schedule`, `idempotency`, `drift_alert`
- Valuation: **Weighted Average** — receipts blend into `avg_unit_cost_minor`, issues cost
  at the current average without changing it; `reconcile_stock` alerts on cache/ledger
  drift instead of silently rewriting it (ADR 023)
- Procure-to-pay: purchase request (draft → submit → Approvals routing → decide callback)
  → purchase order (draft → issue) → goods receipt (draft, partial OK → post: stock
  movement + PO status + one Dr Inventory / Cr AP journal to Finance, all-or-nothing) →
  optional vendor-bill proxy to Finance (`vendor_bills.rs`)
- Fixed assets: CRUD, assign/return (opaque `emp_…` reference, no People FK), straight-line
  depreciation (posts Dr Depreciation Expense / Cr Accumulated Depreciation to Finance),
  maintenance schedules + due list
- Approval routing: `purchase_request` subject type added to `companyos-project`'s
  approvals engine, default policy routes to the `manager` role; decide callback posts to
  `/api/v1/inventory/purchase-requests/{id}/decide`
- Finance: `inventory_receipt` / `inventory_cogs` / `inventory_depreciation` /
  `vendor_bill` / `vendor_payment` added to the journal `source_type` allowlist;
  `finance_vendor_bill` table + `/api/v1/finance/vendor-bills` (create, pay) added
- Cut: multi-warehouse transfer approvals, lot/serial tracking, landed cost allocation,
  cycle-count workflows, barcode/RFID scanning UI — all deferred past this slice

## Later (not this PR)

| Phase | Notes |
|-------|--------|
| InvoiceDunning | Temporal dunning polish |
| PDF / email | Nice-to-have |
| Mobile | Flutter / Tauri |

## Cut order if needed

Cut full statutory filing (TDS/EIN/HMRC), multi-country tax engines, live bank
payouts, and benefits marketplace before immutable runs, traceable payslips,
leave/attendance-aware calc, journal post, self-service, and gated+audited salary reads.
