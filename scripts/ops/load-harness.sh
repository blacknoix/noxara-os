#!/usr/bin/env bash
# Modest in-process load harness. Records p95 against budgets (not production SLOs).
# Budgets: read ≤200ms, write ≤400ms.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

OUT_DIR="${LOAD_HARNESS_OUT:-/tmp/companyos-load-harness}"
mkdir -p "${OUT_DIR}"
REPORT="${OUT_DIR}/results.txt"

echo "==> Load harness (budgets: read p95 ≤200ms, write p95 ≤400ms)"
echo "    Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser)."

export AUTH_JWT_SECRET="${AUTH_JWT_SECRET:-ci-test-auth-secret}"
export AUTH_COOKIE_SECURE="${AUTH_COOKIE_SECURE:-0}"
export COMPANYOS_LOCAL_AUTH="${COMPANYOS_LOCAL_AUTH:-0}"
export HIBP_ENABLED="${HIBP_ENABLED:-0}"
export OPS_LOAD_HARNESS=1

set +e
cargo test -p companyos-core --test ops_load_harness -- --nocapture 2>&1 | tee "${REPORT}"
status=${PIPESTATUS[0]}
set -e

echo "REPORT=${REPORT}"
if [[ ${status} -ne 0 ]]; then
  echo "Load harness exited ${status} (informational CI job may continue-on-error)"
  exit "${status}"
fi
echo "Load harness OK (local budgets only — not production SLOs)"
