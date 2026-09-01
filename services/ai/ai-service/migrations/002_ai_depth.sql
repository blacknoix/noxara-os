-- Phase 3.5 AI depth — proactive insights + meeting summaries (propose-only).

CREATE TABLE IF NOT EXISTS ai_insight (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    insight_type TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    citations JSONB NOT NULL DEFAULT '[]'::jsonb,
    suggested_action JSONB,
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'dismissed', 'accepted_suggestion')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT ai_insight_public_id_uniq UNIQUE (public_id)
);

CREATE INDEX IF NOT EXISTS ai_insight_org_status_idx
    ON ai_insight (org_id, status, created_at DESC);

CREATE INDEX IF NOT EXISTS ai_insight_org_type_idx
    ON ai_insight (org_id, insight_type);

ALTER TABLE ai_insight ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_insight FORCE ROW LEVEL SECURITY;

CREATE POLICY ai_insight_tenant ON ai_insight
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE IF NOT EXISTS ai_meeting_summary (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    public_id TEXT NOT NULL,
    calendar_event_id TEXT NOT NULL,
    calendar_connector TEXT NOT NULL DEFAULT 'calendar.microsoft',
    transcript TEXT,
    summary_markdown TEXT NOT NULL DEFAULT '',
    action_items JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'suggested'
        CHECK (status IN ('suggested', 'accepted', 'rejected')),
    accepted_at TIMESTAMPTZ,
    accepted_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT ai_meeting_summary_public_id_uniq UNIQUE (public_id)
);

CREATE INDEX IF NOT EXISTS ai_meeting_summary_org_idx
    ON ai_meeting_summary (org_id, created_at DESC);

CREATE INDEX IF NOT EXISTS ai_meeting_summary_event_idx
    ON ai_meeting_summary (org_id, calendar_event_id);

ALTER TABLE ai_meeting_summary ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_meeting_summary FORCE ROW LEVEL SECURITY;

CREATE POLICY ai_meeting_summary_tenant ON ai_meeting_summary
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);
