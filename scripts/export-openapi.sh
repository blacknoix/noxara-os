#!/usr/bin/env bash
# Export OpenAPI from core + CRM (offline cargo examples) and merge into packages/sdk.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/packages/sdk/openapi.json"
CORE_TMP="/tmp/companyos-core-openapi.json"
CRM_TMP="/tmp/companyos-crm-openapi.json"

cd "$ROOT"

echo "==> Exporting core OpenAPI (offline)..."
cargo run -p companyos-core --example export_openapi >"$CORE_TMP"

echo "==> Exporting CRM OpenAPI (offline)..."
cargo run -p companyos-crm --example export_openapi >"$CRM_TMP"

export ROOT_OVERRIDE="$ROOT"
python3 <<'PY'
import json
import os
from pathlib import Path

root = Path(os.environ["ROOT_OVERRIDE"])
core = json.loads(Path("/tmp/companyos-core-openapi.json").read_text())
crm = json.loads(Path("/tmp/companyos-crm-openapi.json").read_text())

merged = dict(core)
merged_paths = dict(core.get("paths", {}))
merged_paths.update(crm.get("paths", {}))
merged["paths"] = merged_paths

core_components = core.get("components", {})
crm_components = crm.get("components", {})
merged_components = dict(core_components)
merged_schemas = dict(core_components.get("schemas", {}))
merged_schemas.update(crm_components.get("schemas", {}))
merged_components["schemas"] = merged_schemas
for key, value in crm_components.items():
    if key == "schemas":
        continue
    if key in merged_components and isinstance(merged_components[key], dict) and isinstance(value, dict):
        merged_components[key] = {**merged_components[key], **value}
    else:
        merged_components[key] = value
merged["components"] = merged_components

if "tags" in crm:
    existing = {t.get("name") for t in merged.get("tags", []) if isinstance(t, dict)}
    merged_tags = list(merged.get("tags", []))
    for tag in crm["tags"]:
        if tag.get("name") not in existing:
            merged_tags.append(tag)
            existing.add(tag.get("name"))
    merged["tags"] = merged_tags

out_path = root / "packages/sdk/openapi.json"
out_path.write_text(json.dumps(merged, indent=2) + "\n")
print(f"wrote {out_path} ({len(merged_paths)} paths, {len(merged_schemas)} schemas)")
PY

(cd "$ROOT" && pnpm --filter @companyos/sdk generate)
echo "Exported merged OpenAPI to packages/sdk/openapi.json"
