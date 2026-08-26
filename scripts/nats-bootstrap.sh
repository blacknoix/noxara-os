#!/usr/bin/env bash
# Bootstrap NATS JetStream streams for CompanyOS events (Phase 1.8).
set -euo pipefail

NATS_URL="${NATS_URL:-nats://127.0.0.1:4222}"

echo "==> NATS bootstrap at $NATS_URL"

if ! command -v nats >/dev/null 2>&1; then
  echo "ERROR: nats CLI required. Install from https://github.com/nats-io/natscli"
  exit 1
fi

# Main event stream — subject companyos.>
nats --server="$NATS_URL" stream add COMPANYOS_EVENTS \
  --subjects 'companyos.>' \
  --storage file \
  --retention limits \
  --max-msgs=-1 \
  --max-bytes=-1 \
  --max-age=168h \
  --dupe-window=2m \
  --replicas=1 \
  --discard=old \
  --defaults \
  2>/dev/null || nats --server="$NATS_URL" stream info COMPANYOS_EVENTS >/dev/null

# Dead-letter stream
nats --server="$NATS_URL" stream add COMPANYOS_EVENTS_DLQ \
  --subjects 'companyos.dlq.>' \
  --storage file \
  --retention limits \
  --max-msgs=-1 \
  --max-bytes=-1 \
  --max-age=720h \
  --replicas=1 \
  --discard=old \
  --defaults \
  2>/dev/null || nats --server="$NATS_URL" stream info COMPANYOS_EVENTS_DLQ >/dev/null

# Durable consumer for platform services (notification, search, analytics)
nats --server="$NATS_URL" consumer add COMPANYOS_EVENTS platform-consumers \
  --filter 'companyos.>' \
  --ack explicit \
  --deliver all \
  --max-deliver 5 \
  --wait 30s \
  --replay instant \
  --defaults \
  2>/dev/null || nats --server="$NATS_URL" consumer info COMPANYOS_EVENTS platform-consumers >/dev/null

echo "==> Streams ready: COMPANYOS_EVENTS, COMPANYOS_EVENTS_DLQ; consumer platform-consumers"
