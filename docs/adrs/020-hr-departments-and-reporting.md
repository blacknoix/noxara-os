# ADR 020 — HR departments & reporting lines

Status: Accepted (Phase 2.1)

## Context

People (HR) needs department assignment and a manager/reporting line on employee
records. Workspace already owns `department` (`dep_` public ids, hierarchy via
`parent_id`) and membership may reference `department_id` / `team_id`. There is
no employee entity or reporting-line table in Workspace.

Bounded-context rules forbid HR from becoming a second source of truth for org
structure, and forbid cross-context table joins.

## Decision

1. **Departments stay mastered in Workspace.** HR stores only an opaque
   `department_id` UUID (and optional `department_public_id` text) on
   `people_employee` — the same identifier Workspace uses. HR never creates,
   renames, or soft-deletes departments; callers resolve names via Workspace
   APIs / events. No `people_department` table.

2. **Reporting line is mastered in People.** `people_employee.manager_employee_id`
   references another employee in the same org (self-FK within HR schema).
   Workspace membership `team.lead_user_id` is unrelated and is not mirrored.

3. **User identity is not rebuilt.** `people_employee.user_id` is an opaque link
   to `user_identity` / membership (`usr_`). Onboarding links or invites an
   existing identity; it does not invent a parallel people-user store.

## Consequences

- Directory UIs may show department names by joining client-side (or a future
  read model) against Workspace department APIs — never SQL joins from HR.
- Moving a department in Workspace does not rewrite HR rows; the shared UUID
  continues to resolve.
- Org-chart visualization beyond manager_employee_id is out of scope for 2.1.
