#!/usr/bin/env bash
# Export OpenAPI from a running core (or rebuild committed file via cargo).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
URL="${CORE_OPENAPI_URL:-http://127.0.0.1:8081/api/v1/openapi.json}"
OUT="$ROOT/packages/sdk/openapi.json"

if curl -sf "$URL" -o /tmp/companyos-openapi.json; then
  python3 -c "import json,sys; json.dump(json.load(open('/tmp/companyos-openapi.json')), open('$OUT','w'), indent=2); print(open('$OUT','a').write('\\n') or 'wrote $OUT')"
  (cd "$ROOT" && pnpm --filter @companyos/sdk generate)
  echo "Exported live OpenAPI to packages/sdk/openapi.json"
else
  echo "Core not reachable at $URL — committed openapi.json left unchanged."
  echo "Start core (scripts/dev-up) and re-run, or edit openapi.json to match hello schemas."
  exit 1
fi
