#!/usr/bin/env bash
# Seed one org and two users for local development (Phase 1.1 identity model).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATABASE_URL="${DATABASE_URL:-postgres://companyos:companyos@127.0.0.1:5432/companyos}"
SEED_PASSWORD="${SEED_PASSWORD:-correct-horse-battery}"

mkdir -p "$ROOT/.tmp"

if command -v psql >/dev/null 2>&1; then
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$ROOT/services/core/migrations/001_init.sql"
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$ROOT/services/core/migrations/002_auth.sql"
else
  echo "psql not found — ensure migrations run when core starts"
fi

ORG_UUID="018f0000-0000-7000-8000-000000000001"
USER_OWNER="018f0000-0000-7000-8000-000000000011"
USER_MEMBER="018f0000-0000-7000-8000-000000000012"
MEM_OWNER="018f0000-0000-7000-8000-000000000021"
MEM_MEMBER="018f0000-0000-7000-8000-000000000022"
ORG_PUBLIC="org_${ORG_UUID}"
OWNER_PUBLIC="usr_${USER_OWNER}"
MEMBER_PUBLIC="usr_${USER_MEMBER}"

PASS_HASH="$(cd "$ROOT" && cargo run -q -p companyos-core --example hash_password -- "$SEED_PASSWORD" | tail -n 1)"

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 <<SQL
INSERT INTO organization (id, public_id, name)
VALUES ('${ORG_UUID}', '${ORG_PUBLIC}', 'Acme Demo')
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

INSERT INTO membership (id, org_id, user_id, public_id, role, policy_version)
VALUES
  ('${MEM_OWNER}', '${ORG_UUID}', '${USER_OWNER}', 'mem_${MEM_OWNER}', 'owner', 1),
  ('${MEM_MEMBER}', '${ORG_UUID}', '${USER_MEMBER}', 'mem_${MEM_MEMBER}', 'member', 1)
ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role, revoked_at = NULL;
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
# LOCAL-ONLY headers (COMPANYOS_LOCAL_AUTH=1) remain for hello smoke tests.
EOF

echo "Seeded org ${ORG_PUBLIC} with owner ${OWNER_PUBLIC} and member ${MEMBER_PUBLIC}"
echo "Wrote $ROOT/.tmp/seed.env"
echo "Member login: member@acme.demo / ${SEED_PASSWORD}"
echo "Owner login requires MFA enrollment after password."
