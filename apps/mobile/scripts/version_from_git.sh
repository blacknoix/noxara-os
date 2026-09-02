#!/usr/bin/env bash
# Emit Flutter --build-name / --build-number from git + pubspec.
# Usage: eval "$(./scripts/version_from_git.sh)"
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "${ROOT}/../.." && pwd)"

PUBSPEC="${ROOT}/pubspec.yaml"
VERSION_LINE="$(grep -E '^version:' "${PUBSPEC}" | head -1 | awk '{print $2}')"
BUILD_NAME="${VERSION_LINE%%+*}"
BUILD_NUMBER="$(git -C "${REPO_ROOT}" rev-list --count HEAD 2>/dev/null || echo 1)"

echo "COMPANYOS_BUILD_NAME=${BUILD_NAME}"
echo "COMPANYOS_BUILD_NUMBER=${BUILD_NUMBER}"
