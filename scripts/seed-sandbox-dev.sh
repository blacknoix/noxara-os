#!/usr/bin/env bash
# Seed a sandbox organization + API key for public SDK / fixture tests.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -z "${DATABASE_URL:-}" ]]; then
  export DATABASE_URL="${DATABASE_URL:-postgres://companyos:companyos@127.0.0.1:5432/companyos}"
fi

echo "==> Seeding sandbox via cargo example (core)..."
COMPANYOS_LOCAL_AUTH=1 cargo run -p companyos-core --example seed_sandbox 2>/dev/null || {
  echo "seed_sandbox example missing — writing placeholder env for docs"
  mkdir -p .tmp
  cat > .tmp/sandbox.env <<EOF
# Run migrations + seed_sandbox example to populate real values.
SANDBOX_ORG_ID=org_00000000-0000-0000-0000-000000000001
SANDBOX_API_KEY=replace-me
COMPANYOS_API_URL=http://127.0.0.1:8080
EOF
  exit 0
}
