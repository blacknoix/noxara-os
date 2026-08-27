-- Phase 2.2 Attendance & Leave.
-- Append-only attendance facts and leave ledger; balances derived from ledger.
-- Tenant isolation via org_id + FORCE RLS. Public prefixes: sch_/hol_/att_/lvt_/lvr_/lv_.

-- ---------------------------------------------------------------------------
-- Work schedules (org or location-scoped)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_work_schedule (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    timezone        TEXT NOT NULL DEFAULT 'UTC',
    -- JSON: {"mon":[["09:00","17:00"]], ...} weekday → list of [start,end] local times
    weekly_hours    JSONB NOT NULL DEFAULT '{}'::jsonb,
    location        TEXT,
    is_default      BOOLEAN NOT NULL DEFAULT false,
    owner_user_id   UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_work_schedule_org_idx
    ON people_work_schedule (org_id) WHERE deleted_at IS NULL;
ALTER TABLE people_work_schedule ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_work_schedule FORCE ROW LEVEL SECURITY;
CREATE POLICY people_work_schedule_tenant_isolation ON people_work_schedule
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Holidays (org or location calendar entries)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_holiday (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    holiday_date    DATE NOT NULL,
    location        TEXT,
    is_half_day     BOOLEAN NOT NULL DEFAULT false,
    half_day_period TEXT CHECK (half_day_period IS NULL OR half_day_period IN ('am', 'pm')),
    owner_user_id   UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_holiday_org_date_idx
    ON people_holiday (org_id, holiday_date) WHERE deleted_at IS NULL;
ALTER TABLE people_holiday ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_holiday FORCE ROW LEVEL SECURITY;
CREATE POLICY people_holiday_tenant_isolation ON people_holiday
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Attendance — APPEND-ONLY facts. Corrections are reversing/adjusting inserts.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_attendance (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    employee_id         UUID NOT NULL REFERENCES people_employee(id),
    -- check_in | check_out | break_start | break_end | reversal | adjustment
    entry_kind          TEXT NOT NULL CHECK (entry_kind IN (
        'check_in', 'check_out', 'break_start', 'break_end', 'reversal', 'adjustment'
    )),
    recorded_at         TIMESTAMPTZ NOT NULL,
    local_date          DATE NOT NULL,
    timezone            TEXT NOT NULL DEFAULT 'UTC',
    source              TEXT NOT NULL DEFAULT 'manual' CHECK (source IN (
        'manual', 'geo', 'csv_import', 'system'
    )),
    latitude            DOUBLE PRECISION,
    longitude           DOUBLE PRECISION,
    accuracy_meters     DOUBLE PRECISION,
    note                TEXT,
    -- When this row reverses another attendance fact (append-only correction).
    reverses_id         UUID REFERENCES people_attendance(id),
    -- Import batch key for CSV idempotency (optional).
    import_batch_key    TEXT,
    owner_user_id       UUID NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Intentionally NO updated_at / deleted_at — facts are immutable.
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS people_attendance_org_emp_date_idx
    ON people_attendance (org_id, employee_id, local_date);
CREATE INDEX IF NOT EXISTS people_attendance_org_owner_idx
    ON people_attendance (org_id, owner_user_id);
CREATE INDEX IF NOT EXISTS people_attendance_org_created_idx
    ON people_attendance (org_id, created_at DESC);
ALTER TABLE people_attendance ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_attendance FORCE ROW LEVEL SECURITY;
CREATE POLICY people_attendance_tenant_isolation ON people_attendance
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Reject UPDATE of attendance fact rows (append-only).
CREATE OR REPLACE FUNCTION people_attendance_reject_update()
RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'people_attendance is append-only; corrections must insert reversing/adjusting rows'
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS people_attendance_no_update ON people_attendance;
CREATE TRIGGER people_attendance_no_update
    BEFORE UPDATE ON people_attendance
    FOR EACH ROW EXECUTE FUNCTION people_attendance_reject_update();

-- ---------------------------------------------------------------------------
-- Leave types + accrual policy
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_leave_type (
    id                      UUID PRIMARY KEY,
    org_id                  UUID NOT NULL REFERENCES organization(id),
    public_id               TEXT NOT NULL,
    code                    TEXT NOT NULL,
    name                    TEXT NOT NULL,
    -- annual | sick | unpaid | custom
    category                TEXT NOT NULL CHECK (category IN (
        'annual', 'sick', 'unpaid', 'custom'
    )),
    -- none | monthly | yearly | on_hire
    accrual_cadence         TEXT NOT NULL DEFAULT 'yearly' CHECK (accrual_cadence IN (
        'none', 'monthly', 'yearly', 'on_hire'
    )),
    -- Accrual amount in milli-days (1000 = 1.0 day).
    accrual_units_milli     INT NOT NULL DEFAULT 0,
    -- Carry-forward cap in milli-days (NULL = unlimited).
    carry_forward_cap_milli INT,
    -- Days after year-end before unused carry-forward expires (NULL = no expiry).
    expiry_days             INT,
    allows_half_day         BOOLEAN NOT NULL DEFAULT true,
    requires_approval       BOOLEAN NOT NULL DEFAULT true,
    is_active               BOOLEAN NOT NULL DEFAULT true,
    owner_user_id           UUID NOT NULL,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at              TIMESTAMPTZ,
    version                 INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, code)
);
CREATE INDEX IF NOT EXISTS people_leave_type_org_idx
    ON people_leave_type (org_id) WHERE deleted_at IS NULL;
ALTER TABLE people_leave_type ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_leave_type FORCE ROW LEVEL SECURITY;
CREATE POLICY people_leave_type_tenant_isolation ON people_leave_type
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Leave ledger — source of truth for balances (append-only)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_leave_ledger (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    employee_id         UUID NOT NULL REFERENCES people_employee(id),
    leave_type_id       UUID NOT NULL REFERENCES people_leave_type(id),
    -- accrual | debit | credit | carry_forward | expiry | adjustment
    entry_kind          TEXT NOT NULL CHECK (entry_kind IN (
        'accrual', 'debit', 'credit', 'carry_forward', 'expiry', 'adjustment'
    )),
    -- Signed milli-days: positive increases balance, negative decreases.
    units_milli         INT NOT NULL,
    effective_date      DATE NOT NULL,
    -- Optional expiry for this credit (carry-forward / accrual buckets).
    expires_on          DATE,
    leave_request_id    UUID,
    note                TEXT,
    -- Idempotency / Temporal workflow key (e.g. carry-forward year).
    source_key          TEXT,
    owner_user_id       UUID NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS people_leave_ledger_source_key_uidx
    ON people_leave_ledger (org_id, employee_id, leave_type_id, source_key)
    WHERE source_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS people_leave_ledger_emp_type_idx
    ON people_leave_ledger (org_id, employee_id, leave_type_id, effective_date);
ALTER TABLE people_leave_ledger ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_leave_ledger FORCE ROW LEVEL SECURITY;
CREATE POLICY people_leave_ledger_tenant_isolation ON people_leave_ledger
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE OR REPLACE FUNCTION people_leave_ledger_reject_update()
RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'people_leave_ledger is append-only'
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS people_leave_ledger_no_update ON people_leave_ledger;
CREATE TRIGGER people_leave_ledger_no_update
    BEFORE UPDATE ON people_leave_ledger
    FOR EACH ROW EXECUTE FUNCTION people_leave_ledger_reject_update();

-- ---------------------------------------------------------------------------
-- Leave requests (approval via Operations ApprovalProcess subject_type=leave_request)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_leave_request (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    employee_id         UUID NOT NULL REFERENCES people_employee(id),
    leave_type_id       UUID NOT NULL REFERENCES people_leave_type(id),
    -- draft | pending_approval | approved | rejected | cancelled
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'pending_approval', 'approved', 'rejected', 'cancelled'
    )),
    start_date          DATE NOT NULL,
    end_date            DATE NOT NULL,
    start_period        TEXT NOT NULL DEFAULT 'full' CHECK (start_period IN ('full', 'am', 'pm')),
    end_period          TEXT NOT NULL DEFAULT 'full' CHECK (end_period IN ('full', 'am', 'pm')),
    -- Computed milli-days for the request (timezone-aware calendar math).
    units_milli         INT NOT NULL,
    timezone            TEXT NOT NULL DEFAULT 'UTC',
    reason              TEXT,
    approval_id         TEXT,
    decided_at          TIMESTAMPTZ,
    decision_note       TEXT,
    owner_user_id       UUID NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id),
    CHECK (end_date >= start_date)
);
CREATE INDEX IF NOT EXISTS people_leave_request_org_emp_idx
    ON people_leave_request (org_id, employee_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_leave_request_org_status_idx
    ON people_leave_request (org_id, status) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_leave_request_org_dates_idx
    ON people_leave_request (org_id, start_date, end_date) WHERE deleted_at IS NULL;
ALTER TABLE people_leave_request ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_leave_request FORCE ROW LEVEL SECURITY;
CREATE POLICY people_leave_request_tenant_isolation ON people_leave_request
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- FK from ledger → request (added after both tables exist).
DO $$ BEGIN
    ALTER TABLE people_leave_ledger
        ADD CONSTRAINT people_leave_ledger_request_fk
        FOREIGN KEY (leave_request_id) REFERENCES people_leave_request(id);
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- ---------------------------------------------------------------------------
-- Year-end carry-forward run bookkeeping (Temporal idempotency)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_leave_carry_forward_run (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    year            INT NOT NULL,
    workflow_id     TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'completed' CHECK (status IN (
        'running', 'completed', 'failed'
    )),
    entries_posted  INT NOT NULL DEFAULT 0,
    completed_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, year)
);
ALTER TABLE people_leave_carry_forward_run ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_leave_carry_forward_run FORCE ROW LEVEL SECURITY;
CREATE POLICY people_leave_carry_forward_run_tenant_isolation ON people_leave_carry_forward_run
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
