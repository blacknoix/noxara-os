#!/usr/bin/env bash
# Replay outbox DLQ rows back into outbox_event for republish.
# Usage:
#   scripts/outbox-dlq-replay.sh --all
#   scripts/outbox-dlq-replay.sh --id <uuid>
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 --all | --id <uuid>"
  exit 1
fi

export DATABASE_URL="${DATABASE_URL:-postgres://companyos:companyos@127.0.0.1:5432/companyos}"

exec cargo run -p companyos-outbox-relay -- replay "$@"
