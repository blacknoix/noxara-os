#!/usr/bin/env bash
# Seed one org and two users for local development (Phase 1.2+ workspace model).
# Runs SQL inserts, then OrgProvisioning (roles, pipeline seed stages, expense cats).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATABASE_URL="${DATABASE_URL:-postgres://companyos:companyos@127.0.0.1:5432/companyos}"
SEED_PASSWORD="${SEED_PASSWORD:-correct-horse-battery}"

ORG_UUID="018f0000-0000-7000-8000-000000000001"
USER_OWNER="018f0000-0000-7000-8000-000000000011"
USER_MEMBER="018f0000-0000-7000-8000-000000000012"
MEM_OWNER="018f0000-0000-7000-8000-000000000021"
MEM_MEMBER="018f0000-0000-7000-8000-000000000022"
ORG_PUBLIC="org_${ORG_UUID}"
OWNER_PUBLIC="usr_${USER_OWNER}"
MEMBER_PUBLIC="usr_${USER_MEMBER}"

mkdir -p "$ROOT/.tmp"

if ! command -v psql >/dev/null 2>&1; then
  echo "ERROR: psql is required for seed-dev.sh"
  exit 1
fi

# Ensure pg_trgm exists (CRM duplicate detection). Prefer superuser when available.
if command -v sudo >/dev/null 2>&1; then
  sudo -u postgres psql -d companyos -v ON_ERROR_STOP=1 -c "CREATE EXTENSION IF NOT EXISTS pg_trgm;" 2>/dev/null \
    || PGPASSWORD="${POSTGRES_PASSWORD:-postgres}" psql -h 127.0.0.1 -U postgres -d companyos -v ON_ERROR_STOP=1 \
      -c "CREATE EXTENSION IF NOT EXISTS pg_trgm;" 2>/dev/null \
    || true
fi

echo "==> Applying core migrations (idempotent, advisory-locked)…"
# One session so the lock is held across DDL (same key as companyos_tenancy::SCHEMA_MIGRATION_LOCK_KEY).
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 <<SQL
SELECT pg_advisory_lock(x'00434F534D494701'::bigint);
\i $ROOT/services/core/migrations/001_init.sql
\i $ROOT/services/core/migrations/002_auth.sql
\i $ROOT/services/core/migrations/003_workspace.sql
SELECT pg_advisory_unlock(x'00434F534D494701'::bigint);
SQL

PASS_HASH="$(cd "$ROOT" && cargo run -q -p companyos-core --example hash_password -- "$SEED_PASSWORD" | tail -n 1)"

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 <<SQL
INSERT INTO organization (id, public_id, name, currency, timezone, plan, business_type)
VALUES ('${ORG_UUID}', '${ORG_PUBLIC}', 'Acme Demo', 'USD', 'UTC', 'starter', 'general')
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name;

INSERT INTO user_identity (
  id, public_id, email, email_normalized, password_hash, password_salt,
  display_name, email_verified_at
) VALUES
  ('${USER_OWNER}', '${OWNER_PUBLIC}', 'owner@acme.demo', 'owner@acme.demo',
   '${PASS_HASH}', 'embedded', 'Ada Owner', now()),
  ('${USER_MEMBER}', '${MEMBER_PUBLIC}', 'member@acme.demo', 'member@acme.demo',
   '${PASS_HASH}', 'embedded', 'Sam Member', now())
ON CONFLICT (id) DO UPDATE
  SET email = EXCLUDED.email,
      password_hash = EXCLUDED.password_hash,
      email_verified_at = now();

INSERT INTO app_user (id, public_id, org_id, email, display_name, role)
VALUES
  ('${USER_OWNER}', '${OWNER_PUBLIC}', '${ORG_UUID}', 'owner@acme.demo', 'Ada Owner', 'owner'),
  ('${USER_MEMBER}', '${MEMBER_PUBLIC}', '${ORG_UUID}', 'member@acme.demo', 'Sam Member', 'member')
ON CONFLICT (id) DO UPDATE SET email = EXCLUDED.email;

SELECT set_config('app.org_id', '${ORG_UUID}', false);

INSERT INTO membership (id, org_id, user_id, public_id, role, policy_version, status)
VALUES
  ('${MEM_OWNER}', '${ORG_UUID}', '${USER_OWNER}', 'mem_${MEM_OWNER}', 'owner', 1, 'active'),
  ('${MEM_MEMBER}', '${ORG_UUID}', '${USER_MEMBER}', 'mem_${MEM_MEMBER}', 'member', 1, 'active')
ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role, revoked_at = NULL, status = 'active';
SQL

cat > "$ROOT/.tmp/seed.env" <<EOF
DEV_ORG_UUID=${ORG_UUID}
DEV_ORG_PUBLIC_ID=${ORG_PUBLIC}
DEV_USER_OWNER_UUID=${USER_OWNER}
DEV_USER_OWNER_PUBLIC_ID=${OWNER_PUBLIC}
DEV_USER_MEMBER_UUID=${USER_MEMBER}
DEV_USER_MEMBER_PUBLIC_ID=${MEMBER_PUBLIC}
SEED_PASSWORD=${SEED_PASSWORD}
SEED_OWNER_EMAIL=owner@acme.demo
SEED_MEMBER_EMAIL=member@acme.demo
# Owner requires MFA on login. Member can password-login without MFA.
# LOCAL-ONLY headers (COMPANYOS_LOCAL_AUTH=1) remain for hello smoke tests only.
EOF

echo "==> Running OrgProvisioning (roles, pipeline stages, expense categories)…"
export DATABASE_URL
export DEV_ORG_UUID="$ORG_UUID"
export DEV_USER_OWNER_UUID="$USER_OWNER"
(cd "$ROOT" && cargo run -q -p companyos-core --example seed_dev)

echo "Seeded org ${ORG_PUBLIC} with owner ${OWNER_PUBLIC} and member ${MEMBER_PUBLIC}"
echo "Wrote $ROOT/.tmp/seed.env"
echo "Member login: member@acme.demo / ${SEED_PASSWORD}"
echo "Owner login requires MFA enrollment after password."
echo "Optional sample customer: create via UI or POST /api/v1/sales/customers after services are up."
