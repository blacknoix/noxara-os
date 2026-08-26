-- Bootstrap user (POSTGRES_USER) must remain SUPERUSER.
-- App/tests connect as non-superuser `companyos` so PostgreSQL RLS is enforced
-- (superusers bypass RLS even with FORCE ROW LEVEL SECURITY).

CREATE ROLE companyos LOGIN PASSWORD 'companyos' NOSUPERUSER NOBYPASSRLS CREATEDB;

CREATE DATABASE companyos OWNER companyos;
CREATE DATABASE companyos_test OWNER companyos;

-- pg_trgm is required by CRM duplicate detection. Create as superuser so the
-- non-superuser app role can use similarity() under RLS tests.
\connect companyos
CREATE EXTENSION IF NOT EXISTS pg_trgm;

\connect companyos_test
CREATE EXTENSION IF NOT EXISTS pg_trgm;
