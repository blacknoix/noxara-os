'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  Button,
  EmptyState,
  InlineAlert,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type PackSummary = {
  id: string;
  name: string;
  description: string;
  version: string;
  marketplace_connector_key: string;
  entity_slugs: string[];
  installed: boolean;
};

export default function IndustryPacksPage() {
  const { can, loading } = useCapabilities();
  const canRead = can('custom.builder.read');
  const canInstall = can('custom.package.import');
  const [packs, setPacks] = useState<PackSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    const res = await authFetch('/api/v1/custom/industry-packs');
    if (!res.ok) {
      setError('Could not load industry packs.');
      return;
    }
    const body = (await res.json()) as { items: PackSummary[] };
    setPacks(body.items ?? []);
  }, []);

  useEffect(() => {
    if (!loading && canRead) void refresh();
  }, [loading, canRead, refresh]);

  if (loading) return <p>Loading…</p>;
  if (!canRead) {
    return (
      <PermissionDeniedState
        requiredPermission="custom.builder.read"
        message="Industry packs require custom.builder.read. Installing requires custom.package.import (Member denied by default)."
      />
    );
  }

  async function install(id: string) {
    if (!canInstall) return;
    setBusyId(id);
    setError(null);
    const res = await authFetch(`/api/v1/custom/industry-packs/${id}/install`, {
      method: 'POST',
    });
    setBusyId(null);
    if (!res.ok) {
      const body = await res.text();
      setError(body || 'Install failed');
      return;
    }
    await refresh();
  }

  async function uninstall(id: string) {
    if (!canInstall) return;
    setBusyId(id);
    setError(null);
    const res = await authFetch(`/api/v1/custom/industry-packs/${id}/uninstall`, {
      method: 'POST',
    });
    setBusyId(null);
    if (!res.ok) {
      setError('Uninstall failed');
      return;
    }
    await refresh();
  }

  return (
    <main id="main-content" style={{ padding: '1.5rem', maxWidth: 960 }}>
      <h1 style={{ fontSize: '1.5rem', marginBottom: '0.25rem' }}>Industry packs</h1>
      <p style={{ color: 'var(--cos-color-fg-muted)', marginBottom: '1.5rem' }}>
        Vertical configuration packs (custom entities + seed + marketplace listing). They do not
        fork CRM, finance, HR, or inventory — install is data only.
      </p>

      {error ? (
        <InlineAlert tone="danger" title="Error">
          {error}
        </InlineAlert>
      ) : null}

      {!canInstall ? (
        <InlineAlert tone="warning" title="Install restricted">
          Member cannot install org-wide packs without custom.package.import.
        </InlineAlert>
      ) : null}

      {packs.length === 0 ? (
        <EmptyState title="No packs" description="Industry pack catalogue is empty." />
      ) : (
        <Table
          getRowKey={(row) => row.id}
          columns={[
            {
              key: 'name',
              header: 'Pack',
              cell: (row) => (
                <div>
                  <strong>{row.name}</strong>
                  <div style={{ color: 'var(--cos-color-fg-muted)', fontSize: '0.875rem' }}>
                    {row.description}
                  </div>
                </div>
              ),
            },
            {
              key: 'entities',
              header: 'Entities',
              cell: (row) => row.entity_slugs.join(', '),
            },
            {
              key: 'marketplace',
              header: 'Marketplace',
              cell: (row) => <code>{row.marketplace_connector_key}</code>,
            },
            {
              key: 'status',
              header: 'Status',
              cell: (row) => (row.installed ? 'Installed' : 'Available'),
            },
          ]}
          rows={packs}
          rowActions={(p) =>
            p.installed ? (
              <Button
                size="sm"
                variant="secondary"
                disabled={!canInstall || busyId === p.id}
                onClick={() => void uninstall(p.id)}
              >
                Uninstall
              </Button>
            ) : (
              <Button
                size="sm"
                disabled={!canInstall || busyId === p.id}
                onClick={() => void install(p.id)}
              >
                Install
              </Button>
            )
          }
        />
      )}
    </main>
  );
}
