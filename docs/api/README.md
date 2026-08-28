# API

## Core (`companyos-core`)

- `GET /healthz`, `/livez`, `/readyz`
- `GET|POST /api/v1/hello` (tenant-scoped; LOCAL-ONLY auth)
- `GET /api/v1/dashboard` — dashboard BFF widget snapshot (CRM + Finance + Ops aggregates)
- `GET /api/v1/openapi.json` — core OpenAPI (merged export includes CRM + Finance + Operations)
- `GET /api/v1/workspace/...` — orgs, members, roles, teams, capabilities

## CRM (`companyos-crm`)

Sales bounded context, mounted at **`/api/v1/sales/...`** (proxied by the gateway).

- `GET /api/v1/sales/pipelines` — list pipelines
- `GET /api/v1/sales/pipelines/{id}/board` — kanban board
- `GET|POST /api/v1/sales/customers`, deals, leads, quotes, activities, products
- `GET /api/v1/sales/quotes/{id}/invoice-action` — whether Finance can create an invoice
- `GET /api/v1/sales/reports/summary` — pipeline by stage, win rate, forecast
- `GET /api/v1/sales/openapi.json` — CRM-only OpenAPI document

## Finance (`companyos-finance`)

Finance bounded context, mounted at **`/api/v1/finance/...`** (proxied by the gateway).

- `GET|POST /api/v1/finance/invoices` — drafts; `POST .../issue|send|void`
- `POST /api/v1/finance/invoices/from-quote` — quote snapshot → draft invoice
- `GET|POST /api/v1/finance/payments` — record + allocate
- `POST /api/v1/finance/credit-notes`
- `GET|POST /api/v1/finance/expenses` — submit / decide (approval_limit → Operations approval engine when available)
- `GET|POST /api/v1/operations/approvals` — inbox + create
- `POST /api/v1/operations/approvals/{id}/decide` — Idempotency-Key; duplicate decide is a no-op
- `POST /api/v1/operations/approvals/bulk-decide`
- `GET|POST /api/v1/operations/approval-policies` — versioned policy CRUD (`operations.approval.manage`)
- `POST /api/v1/sales/quotes/{id}/send` — discount ≥ threshold → `pending_approval` hold via approval engine
- `GET /api/v1/finance/reports/summary`
- `GET /api/v1/finance/accounts` — chart of accounts tree; `POST` / `PATCH` manage
- `GET|POST /api/v1/finance/journals` — list + post balanced journals (payroll/manual); period-aware
- `GET|POST /api/v1/finance/periods` — fiscal periods; `.../close`, `.../reopen`, checklist
- `GET|POST /api/v1/finance/bank/accounts` — bank accounts; statement CSV import + auto-match
- `GET|PUT /api/v1/finance/expense-policies` — policy, mileage/per-diem, category limits
- `POST /api/v1/finance/card-transactions/import` + auto-match
- `GET|POST /api/v1/finance/reimbursements` — reimbursement batches (approval-routed)
- `GET /api/v1/finance/reports/trial-balance|profit-and-loss|balance-sheet`
- `POST /api/v1/finance/webhooks/stripe` — idempotent provider fixtures
- `POST /api/v1/finance/events/sales/apply` — in-process CRM event projection (tests)
- `GET /api/v1/finance/openapi.json`

`Idempotency-Key` on POST issue/pay/credit. `If-Match` on draft invoice PATCH only.

## Operations (`companyos-project`)

Projects & Tasks bounded context, mounted at **`/api/v1/operations/...`** (proxied by the gateway).

- `GET|POST /api/v1/operations/projects` — project CRUD (soft delete); `PATCH|DELETE .../{id}`
- `GET|POST /api/v1/operations/tasks` — task CRUD; `POST .../{id}/move` (board); comments / attachments
- `GET /api/v1/operations/board` — five-column kanban (`backlog`…`done`)
- `GET /api/v1/operations/my-work` — assigned tasks + mention intents
- `GET /api/v1/operations/calendar` — due-date events
- `GET /api/v1/operations/summary` — open / overdue / active project counts
- `POST /api/v1/operations/events/sales/apply` — DealWon → project (idempotent)
- `GET /api/v1/operations/openapi.json`

`If-Match` required on task/project PATCH and board move. Mentions notify only users with `operations.task.read`.

## People / HR (`companyos-hr`)

People bounded context, mounted at **`/api/v1/people/...`** (proxied by the gateway).

- `GET|POST /api/v1/people/employees` — directory + create (restricted fields omitted on list)
- `GET|PATCH /api/v1/people/employees/{id}` — detail; sensitive fields require `hr.employee.read_sensitive`
- `GET|PATCH /api/v1/people/me` — self-service non-restricted profile
- `GET|POST /api/v1/people/employees/{id}/compensation` — versioned components (`amount_minor` + currency)
- `GET|POST /api/v1/people/employees/{id}/contracts` — employment contracts
- `GET|POST /api/v1/people/employees/{id}/documents` — documents + expiry (`file_id` via file-service)
- `GET|POST /api/v1/people/employees/{id}/assets` — simple HR asset assignments
- `GET /api/v1/people/employees/{id}/timeline`
- `POST /api/v1/people/employees/onboard` — `Idempotency-Key`; starts EmployeeOnboarding
- `POST /api/v1/people/employees/{id}/offboard` — `Idempotency-Key`; starts EmployeeOffboarding + access revoke
- `GET /api/v1/people/employees/{id}/access-audit` — membership/session checklist
- `GET|POST /api/v1/people/schedules` — work schedules (`sch_`)
- `GET|POST /api/v1/people/holidays` — holiday calendar (`hol_`)
- `GET|POST /api/v1/people/attendance` — append-only attendance (`att_`); `POST .../import` CSV
- `GET /api/v1/people/me/attendance`
- `GET|POST /api/v1/people/leave-types` — leave type catalogue (`lvt_`)
- `GET|POST /api/v1/people/leave-requests` — leave requests (`lvr_`); submit / cancel / decide
- `GET /api/v1/people/me/leave` — self-service leave list
- `GET /api/v1/people/leave/balances` — ledger-derived balances
- `GET /api/v1/people/leave/calendar` — team leave calendar (permission-scoped)
- `GET /api/v1/people/leave/reports/absences` — absence report
- `POST /api/v1/people/leave/carry-forward` — idempotent year-end (`{org}:LeaveCarryForward:{year}`)
- `POST /api/v1/people/leave/accrue` — post accrual ledger entry
- `GET|POST /api/v1/people/payroll/runs` — payroll run lifecycle (`payrun_`)
- `POST /api/v1/people/payroll/runs/{id}/calculate|submit|approve|decide|pay|adjust` — `Idempotency-Key` on calculate/approve/pay
- `GET /api/v1/people/payroll/runs/{id}/payslips` / `.../export` — payslips + CSV payment batch
- `GET /api/v1/people/payroll/payslips/{id}` — audited figure read (`hr.payroll.read`)
- `GET /api/v1/people/me/payslips` — employee self-service (own payslip only)
- `GET|POST /api/v1/people/payroll/components` — configurable earning/deduction components
- `POST /api/v1/finance/journals` — balanced journal post (`finance.journal.post`; payroll source)
- `GET /api/v1/people/openapi.json`

Leave requests with `requires_approval` route through Operations ApprovalProcess (`subject_type=leave_request`).
Payroll submit uses ApprovalProcess (`subject_type=payroll_run`) plus `hr.payroll.approve` on decide (ADR 021).

Gateway URL: same host as core (`PUBLIC_API_URL`), path prefixes `/api/v1/sales`, `/api/v1/finance`, `/api/v1/operations`, and `/api/v1/people`.

Errors use RFC 9457 `application/problem+json` with stable `code` and `request_id`.
