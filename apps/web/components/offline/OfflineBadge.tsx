'use client';

import { Badge, Banner, Button, ConfirmDialog, InlineAlert } from '@companyos/design-system';
import { useState } from 'react';
import { explainConflictRule, resolveConflict } from '../../lib/offline/queue';
import { flushOfflineQueue, useOfflineStatus } from '../../lib/offline/client';

/** Offline / queued-action indicator (03-UIUX eventual-consistency pattern). */
export function OfflineBadge() {
  const { online, queued, conflicts, conflictList, refresh } = useOfflineStatus();
  const [busy, setBusy] = useState(false);
  const [activeConflictId, setActiveConflictId] = useState<string | null>(null);

  const active = conflictList.find((c) => c.id === activeConflictId) ?? null;

  if (online && queued === 0 && conflicts === 0) {
    return null;
  }

  return (
    <div style={{ display: 'grid', gap: '0.5rem', marginBottom: '0.75rem' }}>
      {!online ? (
        <Banner tone="warning">
          <strong>You are offline.</strong> Reads may be cached. Mutations are queued and will
          replay with the same Idempotency-Key when you reconnect.
        </Banner>
      ) : null}

      {online && queued > 0 ? (
        <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem', flexWrap: 'wrap' }}>
          <Badge tone="warning">
            {queued} queued action{queued === 1 ? '' : 's'}
          </Badge>
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              await flushOfflineQueue();
              refresh();
              setBusy(false);
            }}
          >
            Sync now
          </Button>
        </div>
      ) : null}

      {conflicts > 0 ? (
        <InlineAlert tone="danger" title="Sync conflict">
          <p style={{ margin: '0 0 0.5rem' }}>
            {conflicts} change{conflicts === 1 ? '' : 's'} could not apply. Rule:{' '}
            {explainConflictRule()}. The losing edit was not silently dropped.
          </p>
          <ul style={{ margin: 0, paddingLeft: '1.25rem' }}>
            {conflictList.map((c) => (
              <li key={c.id} style={{ marginBottom: '0.35rem' }}>
                <strong>{c.label}</strong> — {c.serverDetail.slice(0, 120)}
                <Button
                  size="sm"
                  variant="secondary"
                  style={{ marginLeft: '0.5rem' }}
                  onClick={() => setActiveConflictId(c.id)}
                >
                  Resolve
                </Button>
              </li>
            ))}
          </ul>
        </InlineAlert>
      ) : null}

      <ConfirmDialog
        open={!!active}
        title="Resolve conflict"
        description={
          active
            ? `${active.label}: server rejected this edit (${active.serverStatus}). Keep the server value (last-write-wins with If-Match).`
            : undefined
        }
        confirmLabel="Keep server value"
        cancelLabel="Dismiss"
        onConfirm={() => {
          if (active) {
            resolveConflict(active.id, 'kept-server');
            refresh();
          }
          setActiveConflictId(null);
        }}
        onClose={() => setActiveConflictId(null)}
      />
    </div>
  );
}
