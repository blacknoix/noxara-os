'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

type LeaveType = {
  id: string;
  code: string;
  name: string;
  category: string;
  accrual_cadence: string;
  accrual_units_milli: number;
  carry_forward_cap_milli: number | null;
  allows_half_day: boolean;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: LeaveType[] };

export default function LeaveTypesPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [code, setCode] = useState('ANNUAL');
  const [name, setName] = useState('Annual leave');
  const [category, setCategory] = useState('annual');
  const [accrual, setAccrual] = useState('20000');
  const [cap, setCap] = useState('5000');
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/people/leave-types');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load leave types.' });
      return;
    }
    const body = (await res.json()) as { items: LeaveType[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setBusy(true);
    try {
      const res = await authFetch('/api/v1/people/leave-types', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          code,
          name,
          category,
          accrual_cadence: 'yearly',
          accrual_units_milli: Number(accrual) || 0,
          carry_forward_cap_milli: cap === '' ? null : Number(cap),
          allows_half_day: true,
          requires_approval: true,
        }),
      });
      if (!res.ok) {
        let message = 'Could not create leave type.';
        try {
          const body = await res.json();
          if (typeof body.detail === 'string') message = body.detail;
        } catch {
          /* ignore */
        }
        setFormError(message);
        return;
      }
      await load();
    } finally {
      setBusy(false);
    }
  }

  if (capsLoading) return <LoadingState label="Loading permissions…" />;
  if (!can('hr.leave.write')) return <PermissionDeniedState requiredPermission="hr.leave.write" />;

  return (
    <main style={pageStyle}>
      <header>
        <p style={eyebrowStyle}>
          <Link href="/people/leave">Leave</Link>
        </p>
        <h1 style={h1Style}>Leave types</h1>
        <p style={leadStyle}>Configure accrual, carry-forward caps, and half-day rules.</p>
      </header>

      <form style={cardStyle} onSubmit={onSubmit} aria-label="Create leave type">
        <h2 style={h2Style}>New leave type</h2>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <label style={labelStyle}>
            Code
            <input value={code} onChange={(e) => setCode(e.target.value)} style={inputStyle} required />
          </label>
          <label style={labelStyle}>
            Name
            <input value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} required />
          </label>
          <label style={labelStyle}>
            Category
            <select value={category} onChange={(e) => setCategory(e.target.value)} style={inputStyle}>
              <option value="annual">Annual</option>
              <option value="sick">Sick</option>
              <option value="unpaid">Unpaid</option>
              <option value="custom">Custom</option>
            </select>
          </label>
          <label style={labelStyle}>
            Yearly accrual (milli-days)
            <input value={accrual} onChange={(e) => setAccrual(e.target.value)} style={inputStyle} />
          </label>
          <label style={labelStyle}>
            Carry-forward cap (milli-days)
            <input value={cap} onChange={(e) => setCap(e.target.value)} style={inputStyle} />
          </label>
        </div>
        {formError ? <p style={errorText}>{formError}</p> : null}
        <Button type="submit" disabled={busy}>
          Create
        </Button>
      </form>

      {state.status === 'loading' ? <LoadingState label="Loading leave types…" /> : null}
      {state.status === 'signed_out' ? (
        <ErrorState title="Sign in required" message="Sign in to manage leave types." />
      ) : null}
      {state.status === 'denied' ? <PermissionDeniedState requiredPermission="hr.leave.write" /> : null}
      {state.status === 'error' ? (
        <ErrorState title="Could not load" message={state.message} />
      ) : null}
      {state.status === 'ready' && state.items.length === 0 ? (
        <EmptyState title="No leave types" description="Create the first leave type above." />
      ) : null}
      {state.status === 'ready' && state.items.length > 0 ? (
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'code', header: 'Code', cell: (r) => r.code },
            { key: 'name', header: 'Name', cell: (r) => r.name },
            { key: 'category', header: 'Category', cell: (r) => r.category },
            {
              key: 'accrual',
              header: 'Accrual',
              cell: (r) => `${(r.accrual_units_milli / 1000).toFixed(1)} / ${r.accrual_cadence}`,
            },
            {
              key: 'cap',
              header: 'CF cap',
              cell: (r) =>
                r.carry_forward_cap_milli == null
                  ? '—'
                  : (r.carry_forward_cap_milli / 1000).toFixed(1),
            },
          ]}
          rows={state.items}
        />
      ) : null}
    </main>
  );
}

const pageStyle: CSSProperties = { display: 'grid', gap: 24, padding: '8px 0 48px' };
const eyebrowStyle: CSSProperties = { margin: 0, fontSize: 13, opacity: 0.7 };
const h1Style: CSSProperties = { margin: '4px 0 8px', fontSize: 28, fontWeight: 650 };
const h2Style: CSSProperties = { margin: '0 0 12px', fontSize: 18, fontWeight: 600 };
const leadStyle: CSSProperties = { margin: 0, maxWidth: 52 * 8, lineHeight: 1.5, opacity: 0.85 };
const cardStyle: CSSProperties = {
  display: 'grid',
  gap: 12,
  padding: 16,
  border: '1px solid color-mix(in srgb, currentColor 14%, transparent)',
  borderRadius: 8,
};
const labelStyle: CSSProperties = { display: 'grid', gap: 6, fontSize: 14 };
const inputStyle: CSSProperties = {
  padding: '8px 10px',
  borderRadius: 6,
  border: '1px solid color-mix(in srgb, currentColor 22%, transparent)',
  background: 'transparent',
  color: 'inherit',
};
const errorText: CSSProperties = { margin: 0, color: 'var(--danger, #b42318)' };
