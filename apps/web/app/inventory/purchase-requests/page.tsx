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

type PurchaseRequest = {
  id: string;
  status: string;
  currency: string;
  total_amount_minor: number;
  notes: string | null;
  lines: { id: string; item_id: string; qty: number }[];
  created_at: string;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: PurchaseRequest[] };

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' {
  if (status === 'approved' || status === 'converted') return 'success';
  if (status === 'pending_approval') return 'warning';
  if (status === 'rejected' || status === 'cancelled') return 'danger';
  return 'neutral';
}

export default function PurchaseRequestsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [currency, setCurrency] = useState('USD');
  const [notes, setNotes] = useState('');
  const [itemId, setItemId] = useState('');
  const [qty, setQty] = useState('1');
  const [unitCost, setUnitCost] = useState('10.00');
  const [submitting, setSubmitting] = useState(false);
  const [actionId, setActionId] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/inventory/purchase-requests?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load purchase requests.' });
      return;
    }
    const body = (await res.json()) as { items: PurchaseRequest[] };
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
      const qtyN = Number.parseInt(qty || '0', 10);
      const unitMinor = Math.round(parseFloat(unitCost || '0') * 100);
      if (!itemId.trim() || qtyN <= 0 || Number.isNaN(unitMinor) || unitMinor < 0) {
        setFormError('Item id, positive qty, and unit cost are required.');
        return;
      }
      const res = await authFetch('/api/v1/inventory/purchase-requests', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          currency: currency.trim().toUpperCase(),
          notes: notes.trim() || null,
          lines: [
            {
              item_id: itemId.trim(),
              qty: qtyN,
              unit_cost_estimate_minor: unitMinor,
            },
          ],
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create purchase request.');
        return;
      }
      setNotes('');
      setItemId('');
      setQty('1');
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  async function onSubmitRequest(id: string) {
    setActionId(id);
    setFormError(null);
    try {
      const res = await authFetch(`/api/v1/inventory/purchase-requests/${encodeURIComponent(id)}/submit`, {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not submit request.');
        return;
      }
      await load();
    } finally {
      setActionId(null);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading purchase requests…" />;
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
    return <PermissionDeniedState requiredPermission="inventory.purchase_request.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Purchase requests unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Inventory</p>
          <h1 style={title}>Purchase requests</h1>
          <p style={muted}>Draft requests, then submit for approval.</p>
        </div>
      </header>

      <form onSubmit={(e) => void onSubmit(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Create request</h2>
        <label style={labelStyle}>
          Item id
          <input
            value={itemId}
            onChange={(e) => setItemId(e.target.value)}
            style={inputStyle}
            placeholder="itm_…"
            required
          />
        </label>
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
        <label style={labelStyle}>
          Notes
          <input value={notes} onChange={(e) => setNotes(e.target.value)} style={inputStyle} />
        </label>
        {formError ? <ErrorState message={formError} /> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Creating…' : 'Create draft'}
        </Button>
      </form>

      {state.items.length === 0 ? (
        <EmptyState title="No purchase requests" description="Create a draft to start procurement." />
      ) : (
        <Table
          getRowKey={(r: PurchaseRequest) => r.id}
          columns={[
            { key: 'id', header: 'Id', cell: (r: PurchaseRequest) => r.id },
            {
              key: 'status',
              header: 'Status',
              cell: (r: PurchaseRequest) => (
                <Badge tone={statusTone(r.status)}>{r.status}</Badge>
              ),
            },
            {
              key: 'lines',
              header: 'Lines',
              cell: (r: PurchaseRequest) => String(r.lines?.length ?? 0),
            },
            {
              key: 'total',
              header: 'Total',
              align: 'right',
              cell: (r: PurchaseRequest) => (
                <MoneyCell amount={r.total_amount_minor / 100} currency={r.currency} />
              ),
            },
            {
              key: 'actions',
              header: '',
              cell: (r: PurchaseRequest) =>
                r.status === 'draft' ? (
                  <Button
                    type="button"
                    variant="secondary"
                    disabled={actionId === r.id}
                    onClick={() => void onSubmitRequest(r.id)}
                  >
                    {actionId === r.id ? 'Submitting…' : 'Submit'}
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
