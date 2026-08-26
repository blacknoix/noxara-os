#!/usr/bin/env bash
# Export OpenAPI from core + CRM + Finance + Operations + platform services
# (offline cargo examples) and merge into packages/sdk.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/packages/sdk/openapi.json"
CORE_TMP="/tmp/companyos-core-openapi.json"
CRM_TMP="/tmp/companyos-crm-openapi.json"
FIN_TMP="/tmp/companyos-finance-openapi.json"
OPS_TMP="/tmp/companyos-project-openapi.json"
NOTIF_TMP="/tmp/companyos-notification-openapi.json"
SEARCH_TMP="/tmp/companyos-search-openapi.json"
FILE_TMP="/tmp/companyos-file-openapi.json"
AI_TMP="/tmp/companyos-ai-openapi.json"

cd "$ROOT"

echo "==> Exporting core OpenAPI (offline)..."
cargo run -p companyos-core --example export_openapi >"$CORE_TMP"

echo "==> Exporting CRM OpenAPI (offline)..."
cargo run -p companyos-crm --example export_openapi >"$CRM_TMP"

echo "==> Exporting Finance OpenAPI (offline)..."
cargo run -p companyos-finance --example export_openapi >"$FIN_TMP"

echo "==> Exporting Operations OpenAPI (offline)..."
cargo run -p companyos-project --example export_openapi >"$OPS_TMP"

echo "==> Exporting Notification OpenAPI (offline)..."
cargo run -p companyos-notification --example export_openapi >"$NOTIF_TMP"

echo "==> Exporting Search OpenAPI (offline)..."
cargo run -p companyos-search --example export_openapi >"$SEARCH_TMP"

echo "==> Exporting File OpenAPI (offline)..."
cargo run -p companyos-file --example export_openapi >"$FILE_TMP"

echo "==> Exporting AI OpenAPI (offline)..."
cargo run -p companyos-ai --example export_openapi >"$AI_TMP"

export ROOT_OVERRIDE="$ROOT"
python3 <<'PY'
import json
import os
from pathlib import Path

root = Path(os.environ["ROOT_OVERRIDE"])
docs = [
    json.loads(Path("/tmp/companyos-core-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-crm-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-finance-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-project-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-notification-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-search-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-file-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-ai-openapi.json").read_text()),
]

merged = dict(docs[0])
merged_paths = dict(docs[0].get("paths", {}))
merged_schemas = dict(docs[0].get("components", {}).get("schemas", {}))
merged_components = dict(docs[0].get("components", {}))
merged_tags = list(docs[0].get("tags", []))
existing_tags = {t.get("name") for t in merged_tags if isinstance(t, dict)}

for doc in docs[1:]:
    merged_paths.update(doc.get("paths", {}))
    comps = doc.get("components", {})
    merged_schemas.update(comps.get("schemas", {}))
    for key, value in comps.items():
        if key == "schemas":
            continue
        if key in merged_components and isinstance(merged_components[key], dict) and isinstance(value, dict):
            merged_components[key] = {**merged_components[key], **value}
        else:
            merged_components[key] = value
    for tag in doc.get("tags", []):
        if tag.get("name") not in existing_tags:
            merged_tags.append(tag)
            existing_tags.add(tag.get("name"))

merged_components["schemas"] = merged_schemas
merged["paths"] = merged_paths
merged["components"] = merged_components
merged["tags"] = merged_tags

out_path = root / "packages/sdk/openapi.json"
out_path.write_text(json.dumps(merged, indent=2) + "\n")
print(f"wrote {out_path} ({len(merged_paths)} paths, {len(merged_schemas)} schemas)")
PY

(cd "$ROOT" && pnpm --filter @companyos/sdk generate)
echo "Exported merged OpenAPI to packages/sdk/openapi.json"
