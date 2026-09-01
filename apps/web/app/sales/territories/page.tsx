'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Territory = {
  id: string;
  name: string;
  description: string | null;
  owner_user_id: string | null;
  created_at: string;
};

export default function TerritoriesPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [items, setItems] = useState<Territory[]>([]);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    const res = await authFetch('/api/v1/sales/territories?limit=100');
    if (res.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!res.ok) {
      setError('Could not load territories');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setItems(body.items ?? []);
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onCreate(e: FormEvent) {
    e.preventDefault();
    if (!can('sales.territory.manage') || !name.trim()) return;
    setSaving(true);
    await authFetch('/api/v1/sales/territories', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ name: name.trim() }),
    });
    setName('');
    setSaving(false);
    await load();
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view territories." />;
  }
  if (capsLoading || loading) return <LoadingState label="Loading territories…" />;
  if (denied || !can('sales.territory.read')) {
    return <PermissionDeniedState requiredPermission="sales.territory.read" />;
  }
  if (error) return <ErrorState title="Territories unavailable" message={error} />;

  return (
    <div style={page}>
      <header>
        <p style={eyebrow}>Sales</p>
        <h1 style={title}>Territories</h1>
        <p style={muted}>Org-scoped assignment of customers and deals — not geo-routing SaaS.</p>
      </header>

      {can('sales.territory.manage') ? (
        <form onSubmit={onCreate} style={form}>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Territory name"
            style={input}
          />
          <Button type="submit" disabled={saving || !name.trim()}>
            Create
          </Button>
        </form>
      ) : null}

      {items.length === 0 ? (
        <EmptyState title="No territories" description="Create a territory to assign customers." />
      ) : (
        <Table
          getRowKey={(t: Territory) => t.id}
          columns={[
            { key: 'name', header: 'Name', cell: (t: Territory) => t.name },
            {
              key: 'desc',
              header: 'Description',
              cell: (t: Territory) => t.description ?? '—',
            },
            { key: 'id', header: 'Id', cell: (t: Territory) => t.id },
          ]}
          rows={items}
        />
      )}
    </div>
  );
}

const page: CSSProperties = { padding: '1.5rem', display: 'grid', gap: '1.25rem' };
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.08em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };
const form: CSSProperties = { display: 'flex', gap: '0.75rem', maxWidth: '28rem' };
const input: CSSProperties = {
  flex: 1,
  padding: '0.5rem 0.75rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg)',
  color: 'var(--cos-color-fg)',
};
