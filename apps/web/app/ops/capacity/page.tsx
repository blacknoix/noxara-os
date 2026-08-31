'use client';

import { useCallback, useEffect, useMemo, useState, type CSSProperties, type FormEvent } from 'react';
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

type Allocation = {
  id: string;
  membership_user_id: string;
  project_id: string | null;
  period_start: string;
  period_end: string;
  capacity_minutes: number;
};

type OverloadRow = {
  member_id: string;
  capacity_minutes: number;
  booked_minutes: number;
  overload_minutes: number;
};

function monthRange(): { from: string; to: string } {
  const now = new Date();
  const from = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1));
  const to = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() + 1, 0));
  return { from: from.toISOString().slice(0, 10), to: to.toISOString().slice(0, 10) };
}

export default function CapacityPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const range = useMemo(() => monthRange(), []);
  const [allocations, setAllocations] = useState<Allocation[]>([]);
  const [overload, setOverload] = useState<OverloadRow[]>([]);
  const [freshnessLag, setFreshnessLag] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [capacityMinutes, setCapacityMinutes] = useState('2400');
  const [memberId, setMemberId] = useState('');
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    const [aRes, oRes, fRes] = await Promise.all([
      authFetch('/api/v1/operations/capacity/allocations'),
      authFetch(
        `/api/v1/operations/capacity/overload?from=${encodeURIComponent(range.from)}&to=${encodeURIComponent(range.to)}`,
      ),
      authFetch('/api/v1/analytics/freshness'),
    ]);
    if (aRes.status === 403 || oRes.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!aRes.ok || !oRes.ok) {
      setError('Could not load capacity');
      setLoading(false);
      return;
    }
    const aBody = await aRes.json();
    const oBody = await oRes.json();
    setAllocations(aBody.items ?? []);
    setOverload(oBody.items ?? []);
    if (fRes.ok) {
      const f = await fRes.json();
      setFreshnessLag(typeof f.lag_seconds === 'number' ? f.lag_seconds : null);
    }
    setLoading(false);
  }, [range.from, range.to]);

  useEffect(() => {
    void load();
  }, [load]);

  async function createAllocation(e: FormEvent) {
    e.preventDefault();
    if (!can('operations.capacity.manage') || !memberId.trim()) return;
    setSaving(true);
    await authFetch('/api/v1/operations/capacity/allocations', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        membership_user_id: memberId.trim(),
        period_start: range.from,
        period_end: range.to,
        capacity_minutes: Number(capacityMinutes),
      }),
    });
    setSaving(false);
    await load();
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view capacity." />;
  }
  if (capsLoading || loading) return <LoadingState label="Loading capacity…" />;
  if (denied || !can('operations.capacity.read')) {
    return <PermissionDeniedState requiredPermission="operations.capacity.read" />;
  }
  if (error) return <ErrorState title="Capacity unavailable" message={error} />;

  return (
    <div style={page}>
      <header>
        <p style={eyebrow}>Operations</p>
        <h1 style={title}>Resource capacity</h1>
        <p style={muted}>
          Allocation vs booked hours ({range.from} → {range.to}). Overload is booked − capacity.
        </p>
        {freshnessLag != null ? (
          <p style={badge}>Analytics freshness lag: {freshnessLag}s</p>
        ) : null}
      </header>

      <section style={section}>
        <h2 style={sectionTitle}>Overload</h2>
        {overload.length === 0 ? (
          <EmptyState title="No overload" description="No booked hours exceed allocated capacity." />
        ) : (
          <Table
            getRowKey={(r: OverloadRow) => r.member_id}
            columns={[
              { key: 'member', header: 'Member', cell: (r: OverloadRow) => r.member_id },
              {
                key: 'cap',
                header: 'Capacity (min)',
                cell: (r: OverloadRow) => String(r.capacity_minutes),
              },
              {
                key: 'booked',
                header: 'Booked (min)',
                cell: (r: OverloadRow) => String(r.booked_minutes),
              },
              {
                key: 'over',
                header: 'Overload (min)',
                cell: (r: OverloadRow) => String(r.overload_minutes),
              },
            ]}
            rows={overload}
          />
        )}
      </section>

      <section style={section}>
        <h2 style={sectionTitle}>Allocations</h2>
        {can('operations.capacity.manage') ? (
          <form onSubmit={createAllocation} style={form}>
            <input
              value={memberId}
              onChange={(e) => setMemberId(e.target.value)}
              placeholder="Member user id (usr_…)"
              style={input}
            />
            <input
              type="number"
              min={1}
              value={capacityMinutes}
              onChange={(e) => setCapacityMinutes(e.target.value)}
              style={{ ...input, maxWidth: '8rem' }}
              aria-label="Capacity minutes"
            />
            <Button type="submit" disabled={saving || !memberId.trim()}>
              Add allocation
            </Button>
          </form>
        ) : null}
        {allocations.length === 0 ? (
          <EmptyState title="No allocations" description="Managers can set member capacity for a period." />
        ) : (
          <Table
            getRowKey={(a: Allocation) => a.id}
            columns={[
              {
                key: 'member',
                header: 'Member',
                cell: (a: Allocation) => a.membership_user_id,
              },
              {
                key: 'period',
                header: 'Period',
                cell: (a: Allocation) => `${a.period_start} → ${a.period_end}`,
              },
              {
                key: 'cap',
                header: 'Capacity (min)',
                cell: (a: Allocation) => String(a.capacity_minutes),
              },
            ]}
            rows={allocations}
          />
        )}
      </section>
    </div>
  );
}

const page: CSSProperties = { padding: '1.5rem', display: 'grid', gap: '1.5rem' };
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.08em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };
const badge: CSSProperties = {
  margin: '0.5rem 0 0',
  fontSize: '0.85rem',
  color: 'var(--cos-color-accent)',
};
const section: CSSProperties = { display: 'grid', gap: '0.75rem' };
const sectionTitle: CSSProperties = { margin: 0, fontSize: '1.1rem' };
const form: CSSProperties = { display: 'flex', gap: '0.5rem', flexWrap: 'wrap' };
const input: CSSProperties = {
  flex: 1,
  minWidth: '12rem',
  padding: '0.5rem 0.75rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg)',
  color: 'var(--cos-color-fg)',
};
