#!/usr/bin/env bash
# Postgres restore drill — dump, restore into isolated DB, assert RLS + journal balance.
# Requires: psql, pg_dump, pg_restore (or psql-compatible custom dump via plain SQL).
# Uses non-superuser for assertions (NOSUPERUSER NOBYPASSRLS).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

PGHOST="${PGHOST:-127.0.0.1}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"
PGPASSWORD="${PGPASSWORD:-postgres}"
export PGHOST PGPORT PGUSER PGPASSWORD

SRC_DB="${RESTORE_DRILL_SRC_DB:-companyos_restore_src}"
DST_DB="${RESTORE_DRILL_DST_DB:-companyos_restore_dst}"
APP_ROLE="${RESTORE_DRILL_APP_ROLE:-companyos_restore}"
APP_PASS="${RESTORE_DRILL_APP_PASS:-companyos_restore}"
DUMP_DIR="${RESTORE_DRILL_DUMP_DIR:-/tmp/companyos-restore-drill}"
DUMP_FILE="${DUMP_DIR}/${SRC_DB}.dump"

echo "==> Restore drill: src=${SRC_DB} dst=${DST_DB} role=${APP_ROLE}"

command -v psql >/dev/null
command -v pg_dump >/dev/null
command -v pg_restore >/dev/null

mkdir -p "${DUMP_DIR}"
rm -f "${DUMP_FILE}"

psql -d postgres -v ON_ERROR_STOP=1 <<SQL
SELECT pg_terminate_backend(pid) FROM pg_stat_activity
  WHERE datname IN ('${SRC_DB}', '${DST_DB}') AND pid <> pg_backend_pid();
DROP DATABASE IF EXISTS ${SRC_DB};
DROP DATABASE IF EXISTS ${DST_DB};
DO \$\$ BEGIN
  CREATE ROLE ${APP_ROLE} LOGIN PASSWORD '${APP_PASS}' NOSUPERUSER NOBYPASSRLS CREATEDB;
EXCEPTION WHEN duplicate_object THEN
  ALTER ROLE ${APP_ROLE} WITH LOGIN PASSWORD '${APP_PASS}' NOSUPERUSER NOBYPASSRLS CREATEDB;
END \$\$;
CREATE DATABASE ${SRC_DB} OWNER ${APP_ROLE};
SQL

# Extensions as superuser (not required for drill schema; skip so dump stays clean)
# pg_trgm is created in app DBs elsewhere — restore drill schema is self-contained.

# Seed schema + two tenants + balanced journal as app role (RLS-aware).
PGPASSWORD="${APP_PASS}" psql -U "${APP_ROLE}" -d "${SRC_DB}" -v ON_ERROR_STOP=1 <<'SQL'
CREATE TABLE organization (
  id UUID PRIMARY KEY,
  public_id TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL
);

CREATE TABLE hello_message (
  id UUID PRIMARY KEY,
  org_id UUID NOT NULL REFERENCES organization(id),
  public_id TEXT NOT NULL UNIQUE,
  message TEXT NOT NULL,
  created_by UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE hello_message ENABLE ROW LEVEL SECURITY;
ALTER TABLE hello_message FORCE ROW LEVEL SECURITY;
CREATE POLICY hello_tenant_isolation ON hello_message
  USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
  WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE finance_journal_entry (
  id UUID PRIMARY KEY,
  org_id UUID NOT NULL REFERENCES organization(id),
  public_id TEXT NOT NULL UNIQUE,
  memo TEXT NOT NULL DEFAULT ''
);
ALTER TABLE finance_journal_entry ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_journal_entry FORCE ROW LEVEL SECURITY;
CREATE POLICY journal_entry_tenant_isolation ON finance_journal_entry
  USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
  WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

CREATE TABLE finance_journal_line (
  id UUID PRIMARY KEY,
  org_id UUID NOT NULL REFERENCES organization(id),
  entry_id UUID NOT NULL REFERENCES finance_journal_entry(id),
  account_code TEXT NOT NULL,
  debit_minor BIGINT NOT NULL DEFAULT 0,
  credit_minor BIGINT NOT NULL DEFAULT 0,
  CHECK (debit_minor >= 0 AND credit_minor >= 0),
  CHECK (NOT (debit_minor > 0 AND credit_minor > 0))
);
ALTER TABLE finance_journal_line ENABLE ROW LEVEL SECURITY;
ALTER TABLE finance_journal_line FORCE ROW LEVEL SECURITY;
CREATE POLICY journal_line_tenant_isolation ON finance_journal_line
  USING (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid)
  WITH CHECK (org_id = NULLIF(current_setting('app.org_id', true), '')::uuid);

-- Stable UUIDs for assertions
INSERT INTO organization (id, public_id, name) VALUES
  ('11111111-1111-7111-8111-111111111111', 'org_restore_a', 'Restore Org A'),
  ('22222222-2222-7222-8222-222222222222', 'org_restore_b', 'Restore Org B');

SELECT set_config('app.org_id', '11111111-1111-7111-8111-111111111111', false);
INSERT INTO hello_message (id, org_id, public_id, message, created_by) VALUES
  ('aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa', '11111111-1111-7111-8111-111111111111',
   'hel_restore_a', 'hello A', 'aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaa1');
INSERT INTO finance_journal_entry (id, org_id, public_id, memo) VALUES
  ('bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb', '11111111-1111-7111-8111-111111111111',
   'jrn_restore_a', 'restore drill balanced');
INSERT INTO finance_journal_line (id, org_id, entry_id, account_code, debit_minor, credit_minor) VALUES
  ('cccccccc-cccc-7ccc-8ccc-ccccccccccc1', '11111111-1111-7111-8111-111111111111',
   'bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb', '1000', 5000, 0),
  ('cccccccc-cccc-7ccc-8ccc-ccccccccccc2', '11111111-1111-7111-8111-111111111111',
   'bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb', '4000', 0, 5000);

SELECT set_config('app.org_id', '22222222-2222-7222-8222-222222222222', false);
INSERT INTO hello_message (id, org_id, public_id, message, created_by) VALUES
  ('dddddddd-dddd-7ddd-8ddd-dddddddddddd', '22222222-2222-7222-8222-222222222222',
   'hel_restore_b', 'hello B', 'dddddddd-dddd-7ddd-8ddd-ddddddddddd1');
SQL

echo "==> Dumping ${SRC_DB}"
pg_dump -Fc -f "${DUMP_FILE}" "${SRC_DB}"

echo "==> Creating isolated restore target ${DST_DB}"
psql -d postgres -v ON_ERROR_STOP=1 <<SQL
CREATE DATABASE ${DST_DB} OWNER ${APP_ROLE};
SQL

echo "==> Restoring into ${DST_DB}"
# --no-owner so objects land as restorer; we reconnect as app role for checks.
pg_restore --no-owner --no-acl --role="${APP_ROLE}" -d "${DST_DB}" "${DUMP_FILE}"

echo "==> Assert non-superuser + RLS + journal balance on restored DB"
PGPASSWORD="${APP_PASS}" psql -U "${APP_ROLE}" -d "${DST_DB}" -v ON_ERROR_STOP=1 <<'SQL'
DO $$
DECLARE
  is_super boolean;
  bypass boolean;
  leaked int;
  debit bigint;
  credit bigint;
BEGIN
  SELECT rolsuper, rolbypassrls INTO is_super, bypass
  FROM pg_roles WHERE rolname = current_user;
  IF is_super OR bypass THEN
    RAISE EXCEPTION 'TENANT ISOLATION SETUP ERROR: role % SUPERUSER=% BYPASSRLS=%',
      current_user, is_super, bypass;
  END IF;

  PERFORM set_config('app.org_id', '11111111-1111-7111-8111-111111111111', false);

  SELECT COUNT(*) INTO leaked FROM hello_message
  WHERE id = 'dddddddd-dddd-7ddd-8ddd-dddddddddddd';
  IF leaked <> 0 THEN
    RAISE EXCEPTION 'TENANT ISOLATION FAILURE: org A session read org B hello after restore';
  END IF;

  SELECT COUNT(*) INTO leaked FROM hello_message
  WHERE org_id = '22222222-2222-7222-8222-222222222222';
  IF leaked <> 0 THEN
    RAISE EXCEPTION 'TENANT ISOLATION FAILURE: planted SELECT leaked org B after restore';
  END IF;

  SELECT COALESCE(SUM(debit_minor),0), COALESCE(SUM(credit_minor),0)
    INTO debit, credit
  FROM finance_journal_line
  WHERE entry_id = 'bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb';
  IF debit <> credit OR debit = 0 THEN
    RAISE EXCEPTION 'JOURNAL INVARIANT FAILURE after restore: debit=% credit=%', debit, credit;
  END IF;

  RAISE NOTICE 'restore drill OK: RLS holds; journal balanced debit=credit=%', debit;
END $$;
SQL

echo "==> Restore drill PASSED (RPO≤5m / RTO≤60m are targets — see docs/ops/rpo-rto-targets.md)"
echo "DUMP=${DUMP_FILE}"
