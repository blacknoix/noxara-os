-- Bootstrap user (POSTGRES_USER) must remain SUPERUSER.
-- App/tests connect as non-superuser `companyos` so PostgreSQL RLS is enforced
-- (superusers bypass RLS even with FORCE ROW LEVEL SECURITY).

CREATE ROLE companyos LOGIN PASSWORD 'companyos' NOSUPERUSER NOBYPASSRLS CREATEDB;

CREATE DATABASE companyos OWNER companyos;
CREATE DATABASE companyos_test OWNER companyos;
