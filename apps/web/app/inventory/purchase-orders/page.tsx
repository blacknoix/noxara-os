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

type PurchaseOrder = {
  id: string;
  supplier_id: string;
  status: string;
  currency: string;
  total_amount_minor: number;
  issued_at: string | null;
  lines: { id: string; item_id: string; qty_ordered: number }[];
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: PurchaseOrder[] };

function statusTone(status: string): 'success' | 'warning' | 'neutral' | 'danger' {
  if (status === 'issued' || status === 'received') return 'success';
  if (status === 'partially_received') return 'warning';
  if (status === 'cancelled') return 'danger';
  return 'neutral';
}

export default function PurchaseOrdersPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [supplierId, setSupplierId] = useState('');
  const [itemId, setItemId] = useState('');
  const [warehouseId, setWarehouseId] = useState('');
  const [qty, setQty] = useState('1');
  const [unitCost, setUnitCost] = useState('10.00');
  const [currency, setCurrency] = useState('USD');
  const [submitting, setSubmitting] = useState(false);
  const [actionId, setActionId] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/inventory/purchase-orders?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load purchase orders.' });
      return;
    }
    const body = (await res.json()) as { items: PurchaseOrder[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onCreate(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setSubmitting(true);
    try {
      const qtyN = Number.parseInt(qty || '0', 10);
      const unitMinor = Math.round(parseFloat(unitCost || '0') * 100);
      if (!supplierId.trim() || !itemId.trim() || !warehouseId.trim() || qtyN <= 0) {
        setFormError('Supplier, item, warehouse, and qty are required.');
        return;
      }
      const res = await authFetch('/api/v1/inventory/purchase-orders', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          supplier_id: supplierId.trim(),
          currency: currency.trim().toUpperCase(),
          lines: [
            {
              item_id: itemId.trim(),
              warehouse_id: warehouseId.trim(),
              qty_ordered: qtyN,
              unit_cost_minor: unitMinor,
            },
          ],
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create purchase order.');
        return;
      }
      setItemId('');
      setQty('1');
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  async function onIssue(id: string) {
    setActionId(id);
    setFormError(null);
    try {
      const res = await authFetch(`/api/v1/inventory/purchase-orders/${encodeURIComponent(id)}/issue`, {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not issue purchase order.');
        return;
      }
      await load();
    } finally {
      setActionId(null);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading purchase orders…" />;
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
    return <PermissionDeniedState requiredPermission="inventory.purchase_order.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Purchase orders unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Inventory</p>
          <h1 style={title}>Purchase orders</h1>
          <p style={muted}>Create draft POs and issue them to suppliers.</p>
        </div>
      </header>

      <form onSubmit={(e) => void onCreate(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Create purchase order</h2>
        <label style={labelStyle}>
          Supplier id
          <input
            value={supplierId}
            onChange={(e) => setSupplierId(e.target.value)}
            style={inputStyle}
            placeholder="sup_…"
            required
          />
        </label>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Item id
            <input value={itemId} onChange={(e) => setItemId(e.target.value)} style={inputStyle} required />
          </label>
          <label style={labelStyle}>
            Warehouse id
            <input
              value={warehouseId}
              onChange={(e) => setWarehouseId(e.target.value)}
              style={inputStyle}
              required
            />
          </label>
        </div>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Qty
            <input value={qty} onChange={(e) => setQty(e.target.value)} style={inputStyle} required />
          </label>
          <label style={labelStyle}>
            Unit cost
            <input
              value={unitCost}
              onChange={(e) => setUnitCost(e.target.value)}
              style={inputStyle}
              inputMode="decimal"
              required
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
        {formError ? <ErrorState message={formError} /> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Creating…' : 'Create draft'}
        </Button>
      </form>

      {state.items.length === 0 ? (
        <EmptyState title="No purchase orders" description="Create a draft PO to issue." />
      ) : (
        <Table
          getRowKey={(r: PurchaseOrder) => r.id}
          columns={[
            { key: 'id', header: 'Id', cell: (r: PurchaseOrder) => r.id },
            { key: 'supplier', header: 'Supplier', cell: (r: PurchaseOrder) => r.supplier_id },
            {
              key: 'status',
              header: 'Status',
              cell: (r: PurchaseOrder) => (
                <Badge tone={statusTone(r.status)}>{r.status}</Badge>
              ),
            },
            {
              key: 'total',
              header: 'Total',
              align: 'right',
              cell: (r: PurchaseOrder) => (
                <MoneyCell amount={r.total_amount_minor / 100} currency={r.currency} />
              ),
            },
            {
              key: 'actions',
              header: '',
              cell: (r: PurchaseOrder) =>
                r.status === 'draft' ? (
                  <Button
                    type="button"
                    variant="secondary"
                    disabled={actionId === r.id}
                    onClick={() => void onIssue(r.id)}
                  >
                    {actionId === r.id ? 'Issuing…' : 'Issue'}
                  </Button>
                ) : (
                  '—'
                ),
            },
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
