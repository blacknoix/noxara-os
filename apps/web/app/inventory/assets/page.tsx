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

type InventoryAsset = {
  id: string;
  name: string;
  asset_tag: string | null;
  status: string;
  acquisition_cost_minor: number;
  currency: string;
  accumulated_depreciation_minor: number;
  useful_life_months: number;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: InventoryAsset[] };

export default function InventoryAssetsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [name, setName] = useState('');
  const [assetTag, setAssetTag] = useState('');
  const [cost, setCost] = useState('100.00');
  const [currency, setCurrency] = useState('USD');
  const [assignAssetId, setAssignAssetId] = useState('');
  const [assigneeId, setAssigneeId] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [actionId, setActionId] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/inventory/assets?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load assets.' });
      return;
    }
    const body = (await res.json()) as { items: InventoryAsset[] };
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
      const costMinor = Math.round(parseFloat(cost || '0') * 100);
      if (!name.trim() || Number.isNaN(costMinor) || costMinor < 0) {
        setFormError('Name and acquisition cost are required.');
        return;
      }
      const res = await authFetch('/api/v1/inventory/assets', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          name: name.trim(),
          asset_tag: assetTag.trim() || null,
          acquisition_cost_minor: costMinor,
          currency: currency.trim().toUpperCase(),
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create asset.');
        return;
      }
      setName('');
      setAssetTag('');
      setCost('100.00');
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  async function onAssign(e: FormEvent) {
    e.preventDefault();
    setActionError(null);
    const id = assignAssetId.trim();
    if (!id || !assigneeId.trim()) {
      setActionError('Asset id and employee public id are required.');
      return;
    }
    setActionId(id);
    try {
      const res = await authFetch(`/api/v1/inventory/assets/${encodeURIComponent(id)}/assign`, {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({ assignee_employee_public_id: assigneeId.trim() }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setActionError(body.detail ?? 'Could not assign asset.');
        return;
      }
      setAssigneeId('');
      await load();
    } finally {
      setActionId(null);
    }
  }

  async function onDepreciate(id: string) {
    setActionId(id);
    setActionError(null);
    try {
      const res = await authFetch(`/api/v1/inventory/assets/${encodeURIComponent(id)}/depreciate`, {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setActionError(body.detail ?? 'Could not depreciate asset.');
        return;
      }
      await load();
    } finally {
      setActionId(null);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading assets…" />;
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
    return <PermissionDeniedState requiredPermission="inventory.asset.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Assets unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Inventory</p>
          <h1 style={title}>Assets</h1>
          <p style={muted}>Fixed asset register — assign and depreciate.</p>
        </div>
      </header>

      <form onSubmit={(e) => void onCreate(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Create asset</h2>
        <label style={labelStyle}>
          Name
          <input value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} required />
        </label>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Asset tag
            <input value={assetTag} onChange={(e) => setAssetTag(e.target.value)} style={inputStyle} />
          </label>
          <label style={labelStyle}>
            Cost
            <input
              value={cost}
              onChange={(e) => setCost(e.target.value)}
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
          {submitting ? 'Creating…' : 'Create asset'}
        </Button>
      </form>

      <form onSubmit={(e) => void onAssign(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Assign asset</h2>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Asset id
            <input
              value={assignAssetId}
              onChange={(e) => setAssignAssetId(e.target.value)}
              style={inputStyle}
              placeholder="ast_…"
              required
            />
          </label>
          <label style={labelStyle}>
            Employee public id
            <input
              value={assigneeId}
              onChange={(e) => setAssigneeId(e.target.value)}
              style={inputStyle}
              placeholder="emp_…"
              required
            />
          </label>
        </div>
        {actionError ? <ErrorState message={actionError} /> : null}
        <Button type="submit" variant="secondary" disabled={actionId !== null}>
          Assign
        </Button>
      </form>

      {state.items.length === 0 ? (
        <EmptyState title="No assets" description="Create a fixed asset to track." />
      ) : (
        <Table
          getRowKey={(r: InventoryAsset) => r.id}
          columns={[
            { key: 'name', header: 'Name', cell: (r: InventoryAsset) => r.name },
            { key: 'tag', header: 'Tag', cell: (r: InventoryAsset) => r.asset_tag ?? '—' },
            {
              key: 'status',
              header: 'Status',
              cell: (r: InventoryAsset) => <Badge>{r.status}</Badge>,
            },
            {
              key: 'cost',
              header: 'Cost',
              align: 'right',
              cell: (r: InventoryAsset) => (
                <MoneyCell amount={r.acquisition_cost_minor / 100} currency={r.currency} />
              ),
            },
            {
              key: 'accum',
              header: 'Accum. dep.',
              align: 'right',
              cell: (r: InventoryAsset) => (
                <MoneyCell
                  amount={r.accumulated_depreciation_minor / 100}
                  currency={r.currency}
                />
              ),
            },
            {
              key: 'actions',
              header: '',
              cell: (r: InventoryAsset) => (
                <Button
                  type="button"
                  variant="ghost"
                  disabled={actionId === r.id || r.status === 'disposed'}
                  onClick={() => void onDepreciate(r.id)}
                >
                  {actionId === r.id ? '…' : 'Depreciate'}
                </Button>
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
