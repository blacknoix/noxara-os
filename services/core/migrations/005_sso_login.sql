-- Phase 2.6 — Enterprise OIDC SSO login path.
--
-- Login start/callback are unauthenticated: the caller does not yet have an
-- `app.org_id` session bound (they are trying to *become* authenticated).
-- Add narrow, SELECT-only, additional permissive RLS policies gated by the
-- `app.sso_lookup` session flag (set only inside the SSO login code path —
-- see `companyos_tenancy::set_sso_lookup`) so `sso_configuration` and
-- `sso_login_state` can be looked up by public id / state hash before the
-- owning org is known. Writes (INSERT/UPDATE/DELETE) still require
-- `app.org_id` via the pre-existing tenant isolation policies — this does
-- **not** widen write access, and it never bypasses membership checks (no
-- god-account SSO auto-provisioning).

DROP POLICY IF EXISTS sso_config_lookup ON sso_configuration;
CREATE POLICY sso_config_lookup ON sso_configuration
    FOR SELECT
    USING (current_setting('app.sso_lookup', true) = '1');

DROP POLICY IF EXISTS sso_login_state_lookup ON sso_login_state;
CREATE POLICY sso_login_state_lookup ON sso_login_state
    FOR SELECT
    USING (current_setting('app.sso_lookup', true) = '1');
