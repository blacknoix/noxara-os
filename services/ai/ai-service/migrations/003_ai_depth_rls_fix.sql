-- Phase 3.5 fix: align ai_insight / ai_meeting_summary RLS with app.org_id.
-- Do not DROP POLICY under FORCE RLS — disable FORCE first, then recreate.

ALTER TABLE ai_insight NO FORCE ROW LEVEL SECURITY;
ALTER TABLE ai_meeting_summary NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS ai_insight_tenant ON ai_insight;
DROP POLICY IF EXISTS ai_meeting_summary_tenant ON ai_meeting_summary;

CREATE POLICY ai_insight_tenant ON ai_insight
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE POLICY ai_meeting_summary_tenant ON ai_meeting_summary
    USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
    WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

ALTER TABLE ai_insight FORCE ROW LEVEL SECURITY;
ALTER TABLE ai_meeting_summary FORCE ROW LEVEL SECURITY;
