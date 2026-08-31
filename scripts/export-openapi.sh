#!/usr/bin/env bash
# Export OpenAPI from core + CRM + Finance + Operations + platform services,
# including analytics (offline cargo examples), and merge into packages/sdk.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/packages/sdk/openapi.json"
CORE_TMP="/tmp/companyos-core-openapi.json"
CRM_TMP="/tmp/companyos-crm-openapi.json"
FIN_TMP="/tmp/companyos-finance-openapi.json"
OPS_TMP="/tmp/companyos-project-openapi.json"
HR_TMP="/tmp/companyos-hr-openapi.json"
NOTIF_TMP="/tmp/companyos-notification-openapi.json"
SEARCH_TMP="/tmp/companyos-search-openapi.json"
FILE_TMP="/tmp/companyos-file-openapi.json"
AI_TMP="/tmp/companyos-ai-openapi.json"
INV_TMP="/tmp/companyos-inventory-openapi.json"
WF_TMP="/tmp/companyos-workflow-openapi.json"
ANALYTICS_TMP="/tmp/companyos-analytics-openapi.json"

cd "$ROOT"

echo "==> Exporting core OpenAPI (offline)..."
cargo run -p companyos-core --example export_openapi >"$CORE_TMP"

echo "==> Exporting CRM OpenAPI (offline)..."
cargo run -p companyos-crm --example export_openapi >"$CRM_TMP"

echo "==> Exporting Finance OpenAPI (offline)..."
cargo run -p companyos-finance --example export_openapi >"$FIN_TMP"

echo "==> Exporting Operations OpenAPI (offline)..."
cargo run -p companyos-project --example export_openapi >"$OPS_TMP"

echo "==> Exporting People/HR OpenAPI (offline)..."
cargo run -p companyos-hr --example export_openapi >"$HR_TMP"

echo "==> Exporting Notification OpenAPI (offline)..."
cargo run -p companyos-notification --example export_openapi >"$NOTIF_TMP"

echo "==> Exporting Search OpenAPI (offline)..."
cargo run -p companyos-search --example export_openapi >"$SEARCH_TMP"

echo "==> Exporting File OpenAPI (offline)..."
cargo run -p companyos-file --example export_openapi >"$FILE_TMP"

echo "==> Exporting AI OpenAPI (offline)..."
cargo run -p companyos-ai --example export_openapi >"$AI_TMP"

echo "==> Exporting Inventory OpenAPI (offline)..."
cargo run -p companyos-inventory --example export_openapi >"$INV_TMP"

echo "==> Exporting Workflow OpenAPI (offline)..."
cargo run -p companyos-workflow --example export_openapi >"$WF_TMP"

echo "==> Exporting Analytics OpenAPI (offline)..."
cargo run -p companyos-analytics --example export_openapi >"$ANALYTICS_TMP"

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
    json.loads(Path("/tmp/companyos-hr-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-notification-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-search-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-file-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-ai-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-inventory-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-workflow-openapi.json").read_text()),
    json.loads(Path("/tmp/companyos-analytics-openapi.json").read_text()),
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

# Filtered public contract for third-party SDKs (Phase 3.3).
PUBLIC_PREFIXES = (
    "/api/v1/sales/customers",
    "/api/v1/sales/deals",
    "/api/v1/sales/quotes",
    "/api/v1/finance/invoices",
    "/api/v1/finance/payments",
    "/api/v1/governance/api-keys",
    "/api/v1/governance/webhooks",
    "/api/v1/openapi.public.json",
)

def is_public(path: str) -> bool:
    return any(path == p or path.startswith(p + "/") or path.startswith(p) for p in PUBLIC_PREFIXES)

public = dict(merged)
public_paths = {k: v for k, v in merged_paths.items() if is_public(k)}
# Mark operations
for path, item in public_paths.items():
    if isinstance(item, dict):
        for method, op in item.items():
            if isinstance(op, dict) and method in ("get", "post", "put", "patch", "delete"):
                op = dict(op)
                op["x-companyos-public"] = True
                item[method] = op
public["paths"] = public_paths
public["info"] = {
    "title": "CompanyOS Public API",
    "version": "v1",
    "description": "Stable public REST surface for third-party integrations (Phase 3.3).",
}
public["x-companyos-deprecation-policy"] = {
    "window_days": 180,
    "dual_publish": True,
    "headers": ["Deprecation", "Sunset", "Link"],
    "doc": "docs/developers/deprecation.md",
}
# Ensure deprecated dual-publish field is annotated when present.
schemas = public.get("components", {}).get("schemas", {})
ex = schemas.get("ApiKeyExchangeResponse", {})
props = ex.get("properties", {})
if "rate_limit_rpm" in props:
    props["rate_limit_rpm"] = dict(props["rate_limit_rpm"])
    props["rate_limit_rpm"]["deprecated"] = True
    props["rate_limit_rpm"]["description"] = (
        "Deprecated alias of rate_limit_per_minute. Dual-published for 180 days."
    )
    ex["properties"] = props
    schemas["ApiKeyExchangeResponse"] = ex
    public.setdefault("components", {})["schemas"] = schemas

public_path = root / "packages/sdk/openapi.public.json"
public_path.write_text(json.dumps(public, indent=2) + "\n")
print(f"wrote {public_path} ({len(public_paths)} public paths)")

# Freeze previous public snapshot for compatibility tests when absent.
baseline = root / "packages/sdk/openapi.public.previous.json"
if not baseline.exists():
    baseline.write_text(json.dumps(public, indent=2) + "\n")
    print(f"froze baseline {baseline}")
PY

(cd "$ROOT" && pnpm --filter @companyos/sdk generate)
(cd "$ROOT" && pnpm --filter @companyos/sdk-python generate || node packages/sdk-python/scripts/generate.mjs)
echo "Exported merged OpenAPI to packages/sdk/openapi.json (+ public)"
