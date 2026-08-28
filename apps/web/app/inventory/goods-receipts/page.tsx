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

type GoodsReceipt = {
  id: string;
  purchase_order_id: string;
  status: string;
  received_at: string | null;
  journal_public_id: string | null;
  lines: { id: string; po_line_id: string; qty_received: number }[];
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: GoodsReceipt[] };

export default function GoodsReceiptsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [purchaseOrderId, setPurchaseOrderId] = useState('');
  const [poLineId, setPoLineId] = useState('');
  const [qtyReceived, setQtyReceived] = useState('1');
  const [submitting, setSubmitting] = useState(false);
  const [actionId, setActionId] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/inventory/goods-receipts?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load goods receipts.' });
      return;
    }
    const body = (await res.json()) as { items: GoodsReceipt[] };
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
      const qtyN = Number.parseInt(qtyReceived || '0', 10);
      if (!purchaseOrderId.trim() || !poLineId.trim() || qtyN <= 0) {
        setFormError('PO id, PO line id, and positive qty are required.');
        return;
      }
      const res = await authFetch('/api/v1/inventory/goods-receipts', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          purchase_order_id: purchaseOrderId.trim(),
          lines: [{ po_line_id: poLineId.trim(), qty_received: qtyN }],
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create goods receipt.');
        return;
      }
      setPoLineId('');
      setQtyReceived('1');
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  async function onPost(id: string) {
    setActionId(id);
    setFormError(null);
    try {
      const res = await authFetch(`/api/v1/inventory/goods-receipts/${encodeURIComponent(id)}/post`, {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not post goods receipt.');
        return;
      }
      await load();
    } finally {
      setActionId(null);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading goods receipts…" />;
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
    return <PermissionDeniedState requiredPermission="inventory.goods_receipt.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Goods receipts unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Inventory</p>
          <h1 style={title}>Goods receipts</h1>
          <p style={muted}>Receive against issued POs, then post to stock and GL.</p>
        </div>
      </header>

      <form onSubmit={(e) => void onCreate(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Create goods receipt</h2>
        <label style={labelStyle}>
          Purchase order id
          <input
            value={purchaseOrderId}
            onChange={(e) => setPurchaseOrderId(e.target.value)}
            style={inputStyle}
            placeholder="po_…"
            required
          />
        </label>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            PO line id
            <input
              value={poLineId}
              onChange={(e) => setPoLineId(e.target.value)}
              style={inputStyle}
              required
            />
          </label>
          <label style={labelStyle}>
            Qty received
            <input
              value={qtyReceived}
              onChange={(e) => setQtyReceived(e.target.value)}
              style={inputStyle}
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
        <EmptyState title="No goods receipts" description="Create a receipt against an issued PO." />
      ) : (
        <Table
          getRowKey={(r: GoodsReceipt) => r.id}
          columns={[
            { key: 'id', header: 'Id', cell: (r: GoodsReceipt) => r.id },
            { key: 'po', header: 'PO', cell: (r: GoodsReceipt) => r.purchase_order_id },
            {
              key: 'status',
              header: 'Status',
              cell: (r: GoodsReceipt) => (
                <Badge tone={r.status === 'posted' ? 'success' : 'neutral'}>{r.status}</Badge>
              ),
            },
            {
              key: 'lines',
              header: 'Lines',
              cell: (r: GoodsReceipt) => String(r.lines?.length ?? 0),
            },
            {
              key: 'journal',
              header: 'Journal',
              cell: (r: GoodsReceipt) => r.journal_public_id ?? '—',
            },
            {
              key: 'actions',
              header: '',
              cell: (r: GoodsReceipt) =>
                r.status === 'draft' ? (
                  <Button
                    type="button"
                    variant="secondary"
                    disabled={actionId === r.id}
                    onClick={() => void onPost(r.id)}
                  >
                    {actionId === r.id ? 'Posting…' : 'Post'}
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
  maxWidth: 520,
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
