-- Bootstrap app role after RDS exists.
-- Run as RDS master (`postgres`) — NEVER make the application connect as master.
--
-- Landmines honored:
-- - App user ≠ superuser / ≠ BYPASSRLS (RLS must enforce)
-- - No DROP POLICY under FORCE RLS from app migrations
-- - PG16: no CREATE POLICY IF NOT EXISTS
-- - Migrations that need advisory locks stay in app migrate paths
--
-- Replace APP_PASSWORD before running. Do not commit real passwords.

DO $$ BEGIN
  CREATE ROLE companyos LOGIN PASSWORD 'APP_PASSWORD' NOSUPERUSER NOBYPASSRLS NOCREATEROLE;
EXCEPTION WHEN duplicate_object THEN
  RAISE NOTICE 'role companyos already exists';
END $$;

GRANT CONNECT ON DATABASE companyos TO companyos;
GRANT CREATE ON DATABASE companyos TO companyos;

-- Temporal uses a separate database on the same instance (fewer moving parts
-- than a second RDS):
--   CREATE DATABASE temporal OWNER postgres;

\connect companyos
CREATE EXTENSION IF NOT EXISTS pg_trgm;
ALTER DATABASE companyos OWNER TO companyos;
