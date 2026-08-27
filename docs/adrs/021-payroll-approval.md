# ADR 021: Payroll run approval (Phase 2.3)

- Status: **Accepted**
- Date: 2026-08-27

## Context

Payroll runs follow `draft → calculate → review → approve → paid`. Leave requests
already route through the Operations approval engine (`ApprovalProcess`). Payroll
needs a clear choice: reuse that engine, or rely solely on `hr.payroll.approve`.

## Decision

**Hybrid — approval engine + explicit `hr.payroll.approve`.**

1. Submitting a calculated run for review creates an Operations approval with
   `subject_type = payroll_run` (default policy: Finance role, escalate Admin).
2. The approval decide callback hits
   `POST /api/v1/people/payroll/runs/{id}/decide`, which requires
   `hr.payroll.approve` and writes audit + outbox (`PayrollRunApproved`).
3. Direct `POST .../approve` with `hr.payroll.approve` remains available for
   local/dev and when the approval engine is unreachable (same authz + audit).

Approved runs are immutable. Corrections are a new adjustment run that
references the original (`adjustment_of_run_id`).

## Consequences

- Finance and Manager defaults include `hr.payroll.approve`; Member does not.
- Journal posting on pay still goes through Finance HTTP APIs (`finance.journal.post`),
  never by writing `finance_*` tables from HR.
- Temporal catalogue entry `PayrollRun` owns long calculate/pay workflows
  (`{org_id}:PayrollRun:{run_id}`).
