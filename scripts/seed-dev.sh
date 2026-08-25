#!/usr/bin/env bash
# Seed one org and two users for local development.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATABASE_URL="${DATABASE_URL:-postgres://companyos:companyos@127.0.0.1:5432/companyos}"

mkdir -p "$ROOT/.tmp"

# Apply schema
python3 - <<PY
import os, subprocess
url = os.environ.get("DATABASE_URL", "postgres://companyos:companyos@127.0.0.1:5432/companyos")
sql = open("$ROOT/services/core/migrations/001_init.sql").read()
# Use psql if available; else sqlx via cargo run later
open("/tmp/companyos_migrate.sql","w").write(sql)
print("migration file ready")
PY

if command -v psql >/dev/null 2>&1; then
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$ROOT/services/core/migrations/001_init.sql"
else
  echo "psql not found — ensure migrations run when core starts"
fi

# Deterministic-ish UUIDv7-like seeds for local docs (valid UUIDs).
ORG_UUID="018f0000-0000-7000-8000-000000000001"
USER_OWNER="018f0000-0000-7000-8000-000000000011"
USER_MEMBER="018f0000-0000-7000-8000-000000000012"
ORG_PUBLIC="org_${ORG_UUID}"
OWNER_PUBLIC="usr_${USER_OWNER}"
MEMBER_PUBLIC="usr_${USER_MEMBER}"

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 <<SQL
INSERT INTO organization (id, public_id, name)
VALUES ('${ORG_UUID}', '${ORG_PUBLIC}', 'Acme Demo')
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name;

INSERT INTO app_user (id, public_id, org_id, email, display_name, role)
VALUES
  ('${USER_OWNER}', '${OWNER_PUBLIC}', '${ORG_UUID}', 'owner@acme.demo', 'Ada Owner', 'owner'),
  ('${USER_MEMBER}', '${MEMBER_PUBLIC}', '${ORG_UUID}', 'member@acme.demo', 'Sam Member', 'member')
ON CONFLICT (id) DO UPDATE SET email = EXCLUDED.email;
SQL

cat > "$ROOT/.tmp/seed.env" <<EOF
DEV_ORG_UUID=${ORG_UUID}
DEV_ORG_PUBLIC_ID=${ORG_PUBLIC}
DEV_USER_OWNER_UUID=${USER_OWNER}
DEV_USER_OWNER_PUBLIC_ID=${OWNER_PUBLIC}
DEV_USER_MEMBER_UUID=${USER_MEMBER}
DEV_USER_MEMBER_PUBLIC_ID=${MEMBER_PUBLIC}
EOF

echo "Seeded org ${ORG_PUBLIC} with owner ${OWNER_PUBLIC} and member ${MEMBER_PUBLIC}"
echo "Wrote $ROOT/.tmp/seed.env"
