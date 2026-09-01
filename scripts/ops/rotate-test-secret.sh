#!/usr/bin/env bash
# Rotate a *test* secret only (MockKms). Never touches production.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "==> Rotating test secret via companyos-crypto MockKms (CI-only)"
cargo test -p companyos-crypto mock_kms_rotate_and_revoke -- --nocapture
echo "==> Test secret rotation OK (see docs/runbooks/secret-rotation.md)"
