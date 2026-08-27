-- Phase 2.3 Payroll (People / HR).
-- Own schema in People context. Journals post via Finance HTTP APIs — never
-- write finance_* tables from HR. Money as BIGINT minor units. FORCE RLS.

-- ---------------------------------------------------------------------------
-- Configurable earning / deduction components (not a country tax engine)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_payroll_component (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    code                TEXT NOT NULL,
    label               TEXT NOT NULL,
    -- earning | deduction
    line_kind           TEXT NOT NULL CHECK (line_kind IN ('earning', 'deduction')),
    -- fixed_from_comp | rate_x_hours | unpaid_leave_proration | percent_of_gross | fixed_amount
    calc_method         TEXT NOT NULL CHECK (calc_method IN (
        'fixed_from_comp',
        'rate_x_hours',
        'unpaid_leave_proration',
        'percent_of_gross',
        'fixed_amount'
    )),
    -- Optional params: percent_bps, fixed_amount_minor, overtime_rate_minor, attendance_kind
    config_json         JSONB NOT NULL DEFAULT '{}'::jsonb,
    currency            CHAR(3),
    is_active           BOOLEAN NOT NULL DEFAULT true,
    sort_order          INT NOT NULL DEFAULT 100,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, code)
);
CREATE INDEX IF NOT EXISTS people_payroll_component_org_idx
    ON people_payroll_component (org_id) WHERE deleted_at IS NULL;
ALTER TABLE people_payroll_component ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_payroll_component FORCE ROW LEVEL SECURITY;
CREATE POLICY people_payroll_component_tenant_isolation ON people_payroll_component
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Payroll runs (immutable once approved)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_payroll_run (
    id                      UUID PRIMARY KEY,
    org_id                  UUID NOT NULL REFERENCES organization(id),
    public_id               TEXT NOT NULL,
    -- draft | calculated | in_review | approved | paid
    status                  TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'calculated', 'in_review', 'approved', 'paid'
    )),
    period_start            DATE NOT NULL,
    period_end              DATE NOT NULL,
    currency                CHAR(3) NOT NULL,
    -- Adjustment runs reference the original approved/paid run.
    adjustment_of_run_id    UUID REFERENCES people_payroll_run(id),
    approval_id             TEXT,
    journal_public_id       TEXT,
    journal_entry_id        UUID,
    -- Aggregates (never log plaintext in application logs).
    employee_count          INT NOT NULL DEFAULT 0,
    gross_minor             BIGINT NOT NULL DEFAULT 0,
    deductions_minor        BIGINT NOT NULL DEFAULT 0,
    net_minor               BIGINT NOT NULL DEFAULT 0,
    calculated_at           TIMESTAMPTZ,
    approved_at             TIMESTAMPTZ,
    paid_at                 TIMESTAMPTZ,
    created_by              UUID NOT NULL,
    owner_user_id           UUID NOT NULL,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at              TIMESTAMPTZ,
    version                 INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id),
    CHECK (period_end >= period_start),
    CHECK (gross_minor >= 0 AND deductions_minor >= 0 AND net_minor >= 0)
);
CREATE INDEX IF NOT EXISTS people_payroll_run_org_status_idx
    ON people_payroll_run (org_id, status) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_payroll_run_org_period_idx
    ON people_payroll_run (org_id, period_start, period_end) WHERE deleted_at IS NULL;
ALTER TABLE people_payroll_run ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_payroll_run FORCE ROW LEVEL SECURITY;
CREATE POLICY people_payroll_run_tenant_isolation ON people_payroll_run
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Approved/paid runs are immutable (status + money + period + journal).
CREATE OR REPLACE FUNCTION people_payroll_run_immutability() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.status IN ('approved', 'paid') THEN
            RAISE EXCEPTION 'approved/paid payroll runs are immutable';
        END IF;
        RETURN OLD;
    END IF;

    -- paid is fully frozen
    IF OLD.status = 'paid' THEN
        RAISE EXCEPTION 'paid payroll runs are immutable';
    END IF;

    -- approved may only transition to paid (journal fields + timestamps)
    IF OLD.status = 'approved' THEN
        IF NEW.status = 'paid'
           AND NEW.period_start = OLD.period_start
           AND NEW.period_end = OLD.period_end
           AND NEW.currency = OLD.currency
           AND NEW.gross_minor = OLD.gross_minor
           AND NEW.deductions_minor = OLD.deductions_minor
           AND NEW.net_minor = OLD.net_minor
           AND NEW.employee_count = OLD.employee_count
           AND NEW.adjustment_of_run_id IS NOT DISTINCT FROM OLD.adjustment_of_run_id
        THEN
            RETURN NEW;
        END IF;
        RAISE EXCEPTION 'approved payroll runs are immutable';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS people_payroll_run_immutability ON people_payroll_run;
CREATE TRIGGER people_payroll_run_immutability
    BEFORE UPDATE OR DELETE ON people_payroll_run
    FOR EACH ROW EXECUTE FUNCTION people_payroll_run_immutability();

-- ---------------------------------------------------------------------------
-- Payslips (one per employee per run)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_payslip (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    run_id              UUID NOT NULL REFERENCES people_payroll_run(id),
    employee_id         UUID NOT NULL REFERENCES people_employee(id),
    currency            CHAR(3) NOT NULL,
    gross_minor         BIGINT NOT NULL DEFAULT 0,
    deductions_minor    BIGINT NOT NULL DEFAULT 0,
    net_minor           BIGINT NOT NULL DEFAULT 0,
    -- draft | issued
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'issued')),
    issued_at           TIMESTAMPTZ,
    distributed_at      TIMESTAMPTZ,
    -- Scope for self-service (linked user or employee owner).
    employee_user_id    UUID,
    owner_user_id       UUID NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    version             INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id),
    UNIQUE (org_id, run_id, employee_id),
    CHECK (gross_minor >= 0 AND deductions_minor >= 0 AND net_minor >= 0)
);
CREATE INDEX IF NOT EXISTS people_payslip_org_run_idx
    ON people_payslip (org_id, run_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_payslip_org_emp_idx
    ON people_payslip (org_id, employee_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS people_payslip_org_user_idx
    ON people_payslip (org_id, employee_user_id)
    WHERE deleted_at IS NULL AND employee_user_id IS NOT NULL;
ALTER TABLE people_payslip ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_payslip FORCE ROW LEVEL SECURITY;
CREATE POLICY people_payslip_tenant_isolation ON people_payslip
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Payslip lines — every figure carries calculation_basis
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS people_payslip_line (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organization(id),
    public_id           TEXT NOT NULL,
    payslip_id          UUID NOT NULL REFERENCES people_payslip(id),
    run_id              UUID NOT NULL REFERENCES people_payroll_run(id),
    -- earning | deduction
    line_kind           TEXT NOT NULL CHECK (line_kind IN ('earning', 'deduction')),
    component_code      TEXT NOT NULL,
    label               TEXT NOT NULL,
    amount_minor        BIGINT NOT NULL,
    currency            CHAR(3) NOT NULL,
    -- Human/machine-readable basis; required for every line (DoD).
    calculation_basis   JSONB NOT NULL,
    sort_order          INT NOT NULL DEFAULT 100,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, public_id),
    CHECK (jsonb_typeof(calculation_basis) = 'object')
);
CREATE INDEX IF NOT EXISTS people_payslip_line_payslip_idx
    ON people_payslip_line (org_id, payslip_id);
CREATE INDEX IF NOT EXISTS people_payslip_line_run_idx
    ON people_payslip_line (org_id, run_id);
ALTER TABLE people_payslip_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE people_payslip_line FORCE ROW LEVEL SECURITY;
CREATE POLICY people_payslip_line_tenant_isolation ON people_payslip_line
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Payslip lines for approved/paid runs must not change.
CREATE OR REPLACE FUNCTION people_payslip_line_guard() RETURNS trigger AS $$
DECLARE
    run_status TEXT;
BEGIN
    SELECT status INTO run_status FROM people_payroll_run
     WHERE id = COALESCE(NEW.run_id, OLD.run_id);
    IF run_status IN ('approved', 'paid') THEN
        RAISE EXCEPTION 'payslip lines for approved/paid runs are immutable';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS people_payslip_line_guard ON people_payslip_line;
CREATE TRIGGER people_payslip_line_guard
    BEFORE INSERT OR UPDATE OR DELETE ON people_payslip_line
    FOR EACH ROW EXECUTE FUNCTION people_payslip_line_guard();
