-- Phase 1.6 Operations (Projects & Tasks) schema.
-- Tenant isolation via org_id + FORCE RLS.
-- customer_id / deal_id are opaque UUIDs (no FK into sales_* tables).
-- Soft delete via deleted_at. Optimistic concurrency via version.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ---------------------------------------------------------------------------
-- Projects
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_project (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    status          TEXT NOT NULL DEFAULT 'active' CHECK (status IN (
        'active', 'on_hold', 'completed', 'cancelled'
    )),
    owner_user_id   UUID NOT NULL,
    -- Opaque links to Sales (never join sales_* tables from this service).
    customer_id     UUID,
    deal_id         UUID,
    deal_public_id  TEXT,
    customer_public_id TEXT,
    starts_at       DATE,
    due_at          DATE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
CREATE INDEX IF NOT EXISTS operations_project_org_idx
    ON operations_project (org_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS operations_project_org_owner_idx
    ON operations_project (org_id, owner_user_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS operations_project_org_deal_idx
    ON operations_project (org_id, deal_id) WHERE deleted_at IS NULL AND deal_id IS NOT NULL;
ALTER TABLE operations_project ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_project FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS operations_project_tenant_isolation ON operations_project
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Tasks — five board states
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_task (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    public_id       TEXT NOT NULL,
    project_id      UUID NOT NULL REFERENCES operations_project(id),
    title           TEXT NOT NULL,
    description     TEXT,
    status          TEXT NOT NULL DEFAULT 'backlog' CHECK (status IN (
        'backlog', 'todo', 'in_progress', 'in_review', 'done'
    )),
    priority        TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN (
        'low', 'medium', 'high', 'urgent'
    )),
    owner_user_id   UUID NOT NULL,
    assignee_id     UUID,
    due_at          TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    labels          TEXT[] NOT NULL DEFAULT '{}',
    position        DOUBLE PRECISION NOT NULL DEFAULT 0,
    board_column    TEXT NOT NULL DEFAULT 'backlog',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INT NOT NULL DEFAULT 1,
    UNIQUE (org_id, public_id)
);
-- My Work / assignee access path: org_id-leading composite for large orgs.
-- Query pattern: WHERE org_id = $1 AND assignee_id = $2 AND deleted_at IS NULL
--   [AND status = …] [AND due_at …] ORDER BY due_at NULLS LAST.
-- Do not full-scan 50k rows in CI — assert this index exists instead.
CREATE INDEX IF NOT EXISTS operations_task_my_work_idx
    ON operations_task (org_id, assignee_id, status, due_at)
    WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS operations_task_org_project_idx
    ON operations_task (org_id, project_id, status)
    WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS operations_task_org_owner_idx
    ON operations_task (org_id, owner_user_id)
    WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS operations_task_org_due_idx
    ON operations_task (org_id, due_at)
    WHERE deleted_at IS NULL AND due_at IS NOT NULL;
ALTER TABLE operations_task ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_task FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS operations_task_tenant_isolation ON operations_task
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Checklist items
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_task_checklist (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    task_id         UUID NOT NULL REFERENCES operations_task(id),
    title           TEXT NOT NULL,
    is_done         BOOLEAN NOT NULL DEFAULT false,
    position        INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS operations_task_checklist_task_idx
    ON operations_task_checklist (org_id, task_id) WHERE deleted_at IS NULL;
ALTER TABLE operations_task_checklist ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_task_checklist FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS operations_task_checklist_tenant_isolation ON operations_task_checklist
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Comments + mention intents (authz-filtered notification targets)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_task_comment (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    task_id         UUID NOT NULL REFERENCES operations_task(id),
    author_user_id  UUID NOT NULL,
    body            TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS operations_task_comment_task_idx
    ON operations_task_comment (org_id, task_id) WHERE deleted_at IS NULL;
ALTER TABLE operations_task_comment ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_task_comment FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS operations_task_comment_tenant_isolation ON operations_task_comment
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Intended mention recipients AFTER authz check. Unauthorized mentions are
-- dropped and never inserted here (no notification-service yet — this is the
-- durable record of who was allowed to be notified).
CREATE TABLE IF NOT EXISTS operations_notification_intent (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    kind            TEXT NOT NULL DEFAULT 'mention',
    resource_type   TEXT NOT NULL,
    resource_id     UUID NOT NULL,
    recipient_user_id UUID NOT NULL,
    actor_user_id   UUID NOT NULL,
    body_preview    TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at    TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS operations_notification_intent_recipient_idx
    ON operations_notification_intent (org_id, recipient_user_id, created_at DESC);
ALTER TABLE operations_notification_intent ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_notification_intent FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS operations_notification_intent_tenant_isolation ON operations_notification_intent
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Attachments (URL/metadata only — no file-service yet; MinIO is infra-only)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_task_attachment (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    task_id         UUID NOT NULL REFERENCES operations_task(id),
    uploaded_by     UUID NOT NULL,
    file_name       TEXT NOT NULL,
    content_type    TEXT,
    byte_size       BIGINT,
    url             TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS operations_task_attachment_task_idx
    ON operations_task_attachment (org_id, task_id) WHERE deleted_at IS NULL;
ALTER TABLE operations_task_attachment ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_task_attachment FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS operations_task_attachment_tenant_isolation ON operations_task_attachment
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Light dependencies (blocked-by only — no Gantt)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_task_dependency (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    task_id         UUID NOT NULL REFERENCES operations_task(id),
    blocked_by_task_id UUID NOT NULL REFERENCES operations_task(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    UNIQUE (org_id, task_id, blocked_by_task_id),
    CHECK (task_id <> blocked_by_task_id)
);
CREATE INDEX IF NOT EXISTS operations_task_dependency_task_idx
    ON operations_task_dependency (org_id, task_id) WHERE deleted_at IS NULL;
ALTER TABLE operations_task_dependency ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_task_dependency FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS operations_task_dependency_tenant_isolation ON operations_task_dependency
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- ---------------------------------------------------------------------------
-- Idempotency
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS operations_idempotency (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organization(id),
    scope           TEXT NOT NULL,
    key             TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body   JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, scope, key)
);
ALTER TABLE operations_idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE operations_idempotency FORCE ROW LEVEL SECURITY;
CREATE POLICY IF NOT EXISTS operations_idempotency_tenant_isolation ON operations_idempotency
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
