'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type InventoryItem = {
  id: string;
  sku: string;
  name: string;
  uom: string;
  currency: string;
  reorder_point_qty: number;
  allow_negative_stock: boolean;
  is_active: boolean;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: InventoryItem[] };

export default function InventoryItemsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [sku, setSku] = useState('');
  const [name, setName] = useState('');
  const [uom, setUom] = useState('ea');
  const [reorderPoint, setReorderPoint] = useState('0');
  const [allowNegative, setAllowNegative] = useState(false);
  const [currency, setCurrency] = useState('USD');
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/inventory/items?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load items.' });
      return;
    }
    const body = (await res.json()) as { items: InventoryItem[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setSubmitting(true);
    try {
      if (!sku.trim() || !name.trim() || !currency.trim()) {
        setFormError('SKU, name, and currency are required.');
        return;
      }
      const res = await authFetch('/api/v1/inventory/items', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          sku: sku.trim(),
          name: name.trim(),
          uom: uom.trim() || 'ea',
          currency: currency.trim().toUpperCase(),
          reorder_point_qty: Number.parseInt(reorderPoint || '0', 10) || 0,
          allow_negative_stock: allowNegative,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create item.');
        return;
      }
      setSku('');
      setName('');
      setUom('ea');
      setReorderPoint('0');
      setAllowNegative(false);
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading items…" />;
  if (state.status === 'signed_out') {
    return (
      <EmptyState
        title="Sign in required"
        action={
          <Link href="/login" style={{ textDecoration: 'none' }}>
            <Button type="button" variant="primary">
              Sign in
            </Button>
          </Link>
        }
      />
    );
  }
  if (state.status === 'denied') {
    return <PermissionDeniedState requiredPermission="inventory.item.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Items unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Inventory</p>
          <h1 style={title}>Items</h1>
          <p style={muted}>SKU master data and reorder settings.</p>
        </div>
      </header>

      <form onSubmit={(e) => void onSubmit(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Create item</h2>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            SKU
            <input value={sku} onChange={(e) => setSku(e.target.value)} style={inputStyle} required />
          </label>
          <label style={labelStyle}>
            Name
            <input value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} required />
          </label>
        </div>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            UOM
            <input value={uom} onChange={(e) => setUom(e.target.value)} style={inputStyle} />
          </label>
          <label style={labelStyle}>
            Reorder point
            <input
              value={reorderPoint}
              onChange={(e) => setReorderPoint(e.target.value)}
              style={inputStyle}
              inputMode="numeric"
            />
          </label>
          <label style={labelStyle}>
            Currency
            <input
              value={currency}
              onChange={(e) => setCurrency(e.target.value.toUpperCase())}
              style={{ ...inputStyle, width: 88 }}
              maxLength={3}
              required
            />
          </label>
        </div>
        <label style={{ ...labelStyle, display: 'flex', alignItems: 'center', gap: 8 }}>
          <input
            type="checkbox"
            checked={allowNegative}
            onChange={(e) => setAllowNegative(e.target.checked)}
          />
          Allow negative stock
        </label>
        {formError ? <ErrorState message={formError} /> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Creating…' : 'Create item'}
        </Button>
      </form>

      {state.items.length === 0 ? (
        <EmptyState title="No items" description="Create an item to get started." />
      ) : (
        <Table
          getRowKey={(r: InventoryItem) => r.id}
          columns={[
            { key: 'sku', header: 'SKU', cell: (r: InventoryItem) => r.sku },
            { key: 'name', header: 'Name', cell: (r: InventoryItem) => r.name },
            { key: 'uom', header: 'UOM', cell: (r: InventoryItem) => r.uom },
            {
              key: 'reorder',
              header: 'Reorder',
              align: 'right',
              cell: (r: InventoryItem) => String(r.reorder_point_qty),
            },
            {
              key: 'neg',
              header: 'Neg. stock',
              cell: (r: InventoryItem) => (
                <Badge tone={r.allow_negative_stock ? 'warning' : 'neutral'}>
                  {r.allow_negative_stock ? 'allowed' : 'blocked'}
                </Badge>
              ),
            },
            {
              key: 'active',
              header: 'Active',
              cell: (r: InventoryItem) => (
                <Badge tone={r.is_active ? 'success' : 'neutral'}>
                  {r.is_active ? 'active' : 'inactive'}
                </Badge>
              ),
            },
            { key: 'ccy', header: 'CCY', cell: (r: InventoryItem) => r.currency },
          ]}
          rows={state.items}
        />
      )}
    </section>
  );
}

const headerStyle: CSSProperties = {
  marginBottom: '1.25rem',
  display: 'flex',
  justifyContent: 'space-between',
  alignItems: 'flex-end',
};
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.06em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const muted: CSSProperties = {
  margin: '0.25rem 0 0',
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};
const formStyle: CSSProperties = {
  display: 'grid',
  gap: 12,
  marginBottom: '1.5rem',
  maxWidth: 560,
};
const labelStyle: CSSProperties = {
  display: 'grid',
  gap: 4,
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};
const inputStyle: CSSProperties = {
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  padding: '0.45rem 0.6rem',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
};
