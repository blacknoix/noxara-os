#!/usr/bin/env bash
# Fail if any alert in docs/ops/alert-catalogue.yaml lacks an existing runbook path.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CATALOGUE="${ROOT}/docs/ops/alert-catalogue.yaml"

if [[ ! -f "${CATALOGUE}" ]]; then
  echo "ERROR: missing ${CATALOGUE}"
  exit 1
fi

python3 - <<'PY' "${CATALOGUE}" "${ROOT}"
import re, sys
from pathlib import Path

catalogue = Path(sys.argv[1])
root = Path(sys.argv[2])
text = catalogue.read_text()

# Minimal YAML scrape: "- id:" and "runbook:" lines (no PyYAML dependency).
alerts = []
current = None
for line in text.splitlines():
    m_id = re.match(r"^\s*-\s*id:\s*(\S+)\s*$", line)
    if m_id:
        current = {"id": m_id.group(1), "runbook": None}
        alerts.append(current)
        continue
    m_rb = re.match(r"^\s*runbook:\s*(\S+)\s*$", line)
    if m_rb and current is not None:
        current["runbook"] = m_rb.group(1)

if not alerts:
    print("ERROR: no alerts parsed from catalogue")
    sys.exit(1)

errors = []
seen = set()
for a in alerts:
    aid = a["id"]
    if aid in seen:
        errors.append(f"duplicate alert id: {aid}")
    seen.add(aid)
    rb = a.get("runbook")
    if not rb:
        errors.append(f"alert {aid}: missing runbook")
        continue
    path = root / rb
    if not path.is_file():
        errors.append(f"alert {aid}: runbook missing at {rb}")

# Inverse soft check: catalogue should cover TRD 8.5 required set.
required = {
    "outbox_lag",
    "dlq_depth",
    "nats_down",
    "replication_event_lag",
    "slo_burn",
    "ai_spend_anomaly",
}
missing_req = required - seen
if missing_req:
    errors.append(f"TRD 8.5 alerts missing from catalogue: {sorted(missing_req)}")

if errors:
    print("ALERT↔RUNBOOK CHECK FAILED:")
    for e in errors:
        print(f"  - {e}")
    sys.exit(1)

print(f"alert↔runbook check OK ({len(alerts)} alerts)")
PY
