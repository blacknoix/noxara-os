# ADR 017: Durable OrgProvisioning command (Temporal follow-up)

- Status: **Accepted**
- Date: 2026-08-25

## Context

Phase 1.2 requires every new organization to be provisioned into a usable workspace
(system roles, settings defaults, seed stubs) without manual steps. ADR 004 says
long-running processes are Temporal workflows. This repository's docker-compose
includes Temporal, but **no Temporal worker host is wired into `services/` yet**.

## Decision

Implement OrgProvisioning as a **durable command** in `workspace_command` with
idempotency key:

```text
{org_public_id}:OrgProvisioning:{org_public_id}
```

Commands are written in the same transaction path as organization creation and
processed immediately by `workspace::provisioning` (not a status column + cron).
Failed commands remain queryable and can be re-driven via `process_pending`.

When a Temporal worker is introduced, keep the same workflow/idempotency ID and
move execution into a Temporal activity that calls the same seed function.

## Consequences

- Phase 1.2 DoD is met without faking Temporal.
- Follow-up: add a Temporal worker that claims `OrgProvisioning` commands and
  preserves the idempotency key above.
