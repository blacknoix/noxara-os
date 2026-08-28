'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type StockMovement = {
  id: string;
  warehouse_id: string;
  item_id: string;
  qty_delta: number;
  unit_cost_minor: number;
  currency: string;
  movement_type: string;
  memo?: string | null;
  created_at: string;
  low_stock?: boolean;
};

type StockLevel = {
  warehouse_id: string;
  item_id: string;
  qty_on_hand: number;
  avg_unit_cost_minor: number;
  last_movement_at?: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; movements: StockMovement[] };

export default function StockPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [stockItemId, setStockItemId] = useState('');
  const [levels, setLevels] = useState<StockLevel[]>([]);
  const [levelsError, setLevelsError] = useState<string | null>(null);
  const [reconcileMsg, setReconcileMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/inventory/movements?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load stock movements.' });
      return;
    }
    const body = (await res.json()) as { items: StockMovement[] };
    setState({ status: 'ready', movements: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function loadLevels(e: FormEvent) {
    e.preventDefault();
    setLevelsError(null);
    if (!stockItemId.trim()) {
      setLevelsError('Item id is required.');
      return;
    }
    setBusy(true);
    try {
      const res = await authFetch(
        `/api/v1/inventory/items/${encodeURIComponent(stockItemId.trim())}/stock`,
      );
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setLevelsError(body.detail ?? 'Could not load stock levels.');
        return;
      }
      const body = (await res.json()) as { items: StockLevel[] };
      setLevels(body.items ?? []);
    } finally {
      setBusy(false);
    }
  }

  async function onReconcile() {
    setReconcileMsg(null);
    setBusy(true);
    try {
      const res = await authFetch('/api/v1/inventory/stock/reconcile', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setReconcileMsg(body.detail ?? 'Reconcile failed.');
        return;
      }
      const body = (await res.json()) as { checked: number; drift_count: number };
      setReconcileMsg(`Checked ${body.checked}; drift alerts: ${body.drift_count}.`);
      await load();
    } finally {
      setBusy(false);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading stock…" />;
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
    return <PermissionDeniedState requiredPermission="inventory.stock.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Stock unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Inventory</p>
          <h1 style={title}>Stock</h1>
          <p style={muted}>Movements ledger, levels by item, and cache reconciliation.</p>
        </div>
        <Button type="button" variant="secondary" disabled={busy} onClick={() => void onReconcile()}>
          {busy ? 'Working…' : 'Reconcile stock'}
        </Button>
      </header>

      {reconcileMsg ? <p style={muted}>{reconcileMsg}</p> : null}

      <form onSubmit={(e) => void loadLevels(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Stock levels by item</h2>
        <label style={labelStyle}>
          Item id
          <input
            value={stockItemId}
            onChange={(e) => setStockItemId(e.target.value)}
            style={inputStyle}
            placeholder="itm_…"
          />
        </label>
        {levelsError ? <ErrorState message={levelsError} /> : null}
        <Button type="submit" variant="primary" disabled={busy}>
          Load levels
        </Button>
      </form>

      {levels.length > 0 ? (
        <div style={{ marginBottom: '1.5rem' }}>
          <Table
            getRowKey={(r: StockLevel) => `${r.warehouse_id}:${r.item_id}`}
            columns={[
              { key: 'wh', header: 'Warehouse', cell: (r: StockLevel) => r.warehouse_id },
              {
                key: 'qty',
                header: 'On hand',
                align: 'right',
                cell: (r: StockLevel) => String(r.qty_on_hand),
              },
              {
                key: 'avg',
                header: 'Avg cost',
                align: 'right',
                cell: (r: StockLevel) => (
                  <MoneyCell amount={r.avg_unit_cost_minor / 100} currency="USD" />
                ),
              },
            ]}
            rows={levels}
          />
        </div>
      ) : null}

      <h2 style={{ margin: '0 0 0.75rem', fontSize: '1.1rem' }}>Recent movements</h2>
      {state.movements.length === 0 ? (
        <EmptyState title="No movements" description="Stock movements appear here." />
      ) : (
        <Table
          getRowKey={(r: StockMovement) => r.id}
          columns={[
            {
              key: 'when',
              header: 'When',
              cell: (r: StockMovement) => r.created_at.slice(0, 19),
            },
            {
              key: 'type',
              header: 'Type',
              cell: (r: StockMovement) => <Badge>{r.movement_type}</Badge>,
            },
            { key: 'item', header: 'Item', cell: (r: StockMovement) => r.item_id },
            { key: 'wh', header: 'Warehouse', cell: (r: StockMovement) => r.warehouse_id },
            {
              key: 'qty',
              header: 'Qty Δ',
              align: 'right',
              cell: (r: StockMovement) => String(r.qty_delta),
            },
            {
              key: 'cost',
              header: 'Unit cost',
              align: 'right',
              cell: (r: StockMovement) => (
                <MoneyCell amount={r.unit_cost_minor / 100} currency={r.currency} />
              ),
            },
            {
              key: 'low',
              header: 'Low',
              cell: (r: StockMovement) =>
                r.low_stock ? <Badge tone="warning">low</Badge> : '—',
            },
          ]}
          rows={state.movements}
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
  gap: 12,
  flexWrap: 'wrap',
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
  margin: '0.25rem 0 0.75rem',
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};
const formStyle: CSSProperties = {
  display: 'grid',
  gap: 12,
  marginBottom: '1.5rem',
  maxWidth: 480,
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
