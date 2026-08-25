-- POSTGRES_USER is a superuser by default; RLS is bypassed for superusers even with
-- FORCE ROW LEVEL SECURITY. Demote for local app/test connections.
ALTER ROLE companyos WITH NOSUPERUSER NOBYPASSRLS;

CREATE DATABASE companyos_test;
GRANT ALL PRIVILEGES ON DATABASE companyos_test TO companyos;
