-- Phase 3.5 Operations depth — timesheets, time entries, capacity.
-- Tenant isolation via org_id + FORCE RLS.
-- Hours stored as minutes INT (60 = 1.00 hour).

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ---------------------------------------------------------------------------
-- Timesheets (week sheets; week_start is Monday)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_timesheet (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    membership_user_id  UUID NOT NULL,
    week_start          DATE NOT NULL,
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'submitted', 'approved', 'rejected'
    )),
    submitted_at        TIMESTAMPTZ,
    approved_at         TIMESTAMPTZ,
    approved_by         UUID,
    approval_id         TEXT,
    notes               TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, membership_user_id, week_start)
);
CREATE INDEX IF NOT EXISTS operations_timesheet_org_user_idx
    ON operations_timesheet (org_id, membership_user_id, week_start DESC);
CREATE INDEX IF NOT EXISTS operations_timesheet_org_status_idx
    ON operations_timesheet (org_id, status, week_start DESC);
ALTER TABLE operations_timesheet ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_timesheet FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_timesheet_tenant_isolation ON operations_timesheet
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Time entries (foundation; optionally linked to a timesheet week)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_time_entry (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    membership_user_id  UUID NOT NULL,
    project_id          UUID NOT NULL REFERENCES operations_project(id),
    task_id             UUID REFERENCES operations_task(id),
    entry_date          DATE NOT NULL,
    minutes             INT NOT NULL CHECK (minutes > 0),
    billable            BOOLEAN NOT NULL DEFAULT true,
    notes               TEXT,
    timesheet_id        UUID REFERENCES operations_timesheet(id),
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'submitted', 'approved', 'rejected'
    )),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS operations_time_entry_org_user_date_idx
    ON operations_time_entry (org_id, membership_user_id, entry_date);
CREATE INDEX IF NOT EXISTS operations_time_entry_org_timesheet_idx
    ON operations_time_entry (org_id, timesheet_id)
    WHERE timesheet_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS operations_time_entry_org_project_idx
    ON operations_time_entry (org_id, project_id, entry_date);
ALTER TABLE operations_time_entry ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_time_entry FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_time_entry_tenant_isolation ON operations_time_entry
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Capacity allocations
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_capacity_allocation (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    membership_user_id  UUID NOT NULL,
    project_id          UUID REFERENCES operations_project(id),
    period_start        DATE NOT NULL,
    period_end          DATE NOT NULL,
    capacity_minutes    INT NOT NULL CHECK (capacity_minutes > 0),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    CHECK (period_end >= period_start)
);
CREATE INDEX IF NOT EXISTS operations_capacity_allocation_org_user_idx
    ON operations_capacity_allocation (org_id, membership_user_id, period_start, period_end);
ALTER TABLE operations_capacity_allocation ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_capacity_allocation FORCE ROW LEVEL SECURITY;
CREATE POLICY operations_capacity_allocation_tenant_isolation
    ON operations_capacity_allocation
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Widen approval policy subject_type to include timesheet
-- ---------------------------------------------------------------------------
ALTER TABLE operations_approval_policy DROP CONSTRAINT IF EXISTS operations_approval_policy_subject_type_check;
ALTER TABLE operations_approval_policy ADD CONSTRAINT operations_approval_policy_subject_type_check
    CHECK (subject_type IN (
        'expense', 'quote_discount', 'generic',
        'leave_request', 'payroll_run', 'purchase_request',
        'timesheet'
    ));
