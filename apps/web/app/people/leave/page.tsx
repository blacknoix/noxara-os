'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type LeaveType = { id: string; code: string; name: string; allows_half_day: boolean };
type LeaveRequest = {
  id: string;
  leave_type_id: string;
  status: string;
  start_date: string;
  end_date: string;
  start_period: string;
  end_period: string;
  units_days: string;
  reason: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; items: LeaveRequest[]; types: LeaveType[] };

export default function MyLeavePage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [leaveTypeId, setLeaveTypeId] = useState('');
  const [startDate, setStartDate] = useState('');
  const [endDate, setEndDate] = useState('');
  const [startPeriod, setStartPeriod] = useState('full');
  const [endPeriod, setEndPeriod] = useState('full');
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const [leaveRes, typesRes] = await Promise.all([
      authFetch('/api/v1/people/me/leave?limit=50'),
      authFetch('/api/v1/people/leave-types'),
    ]);
    const requestId = leaveRes.headers.get('x-request-id') ?? undefined;
    if (leaveRes.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (leaveRes.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!leaveRes.ok) {
      setState({ status: 'error', message: 'Could not load leave.', requestId });
      return;
    }
    const leaveBody = (await leaveRes.json()) as { items: LeaveRequest[] };
    const typesBody = typesRes.ok
      ? ((await typesRes.json()) as { items: LeaveType[] })
      : { items: [] as LeaveType[] };
    if (!leaveTypeId && typesBody.items[0]) setLeaveTypeId(typesBody.items[0].id);
    setState({ status: 'ready', items: leaveBody.items ?? [], types: typesBody.items ?? [] });
  }, [leaveTypeId]);

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    if (!leaveTypeId || !startDate || !endDate) {
      setFormError('Leave type and dates are required.');
      return;
    }
    setBusy(true);
    try {
      const res = await authFetch('/api/v1/people/leave-requests', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          leave_type_id: leaveTypeId,
          start_date: startDate,
          end_date: endDate,
          start_period: startPeriod,
          end_period: endPeriod,
          reason: reason.trim() || null,
          submit: true,
        }),
      });
      if (!res.ok) {
        let message = 'Could not request leave.';
        try {
          const body = await res.json();
          if (typeof body.detail === 'string') message = body.detail;
        } catch {
          /* ignore */
        }
        setFormError(message);
        return;
      }
      setReason('');
      await load();
    } finally {
      setBusy(false);
    }
  }

  if (capsLoading) return <LoadingState label="Loading permissions…" />;
  if (!can('hr.leave.read')) return <PermissionDeniedState requiredPermission="hr.leave.read" />;

  return (
    <main style={pageStyle}>
      <header>
        <p style={eyebrowStyle}>
          <Link href="/people">People</Link>
        </p>
        <h1 style={h1Style}>My leave</h1>
        <p style={leadStyle}>Request time off. Approvals go through the existing approval engine.</p>
      </header>

      {can('hr.leave.write') && state.status === 'ready' ? (
        <form style={cardStyle} onSubmit={onSubmit} aria-label="Request leave">
          <h2 style={h2Style}>Request leave</h2>
          <label style={labelStyle}>
            Leave type
            <select
              value={leaveTypeId}
              onChange={(e) => setLeaveTypeId(e.target.value)}
              style={inputStyle}
            >
              {state.types.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.code} — {t.name}
                </option>
              ))}
            </select>
          </label>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
            <label style={labelStyle}>
              Start
              <input
                type="date"
                value={startDate}
                onChange={(e) => setStartDate(e.target.value)}
                style={inputStyle}
                required
              />
            </label>
            <label style={labelStyle}>
              End
              <input
                type="date"
                value={endDate}
                onChange={(e) => setEndDate(e.target.value)}
                style={inputStyle}
                required
              />
            </label>
            <label style={labelStyle}>
              Start period
              <select
                value={startPeriod}
                onChange={(e) => setStartPeriod(e.target.value)}
                style={inputStyle}
              >
                <option value="full">Full day</option>
                <option value="am">AM</option>
                <option value="pm">PM</option>
              </select>
            </label>
            <label style={labelStyle}>
              End period
              <select
                value={endPeriod}
                onChange={(e) => setEndPeriod(e.target.value)}
                style={inputStyle}
              >
                <option value="full">Full day</option>
                <option value="am">AM</option>
                <option value="pm">PM</option>
              </select>
            </label>
          </div>
          <label style={labelStyle}>
            Reason
            <input value={reason} onChange={(e) => setReason(e.target.value)} style={inputStyle} />
          </label>
          {formError ? <p style={errorText}>{formError}</p> : null}
          <Button type="submit" disabled={busy || state.types.length === 0}>
            Submit request
          </Button>
        </form>
      ) : null}

      {state.status === 'loading' ? <LoadingState label="Loading leave…" /> : null}
      {state.status === 'signed_out' ? (
        <ErrorState title="Sign in required" message="Sign in to view leave." />
      ) : null}
      {state.status === 'denied' ? <PermissionDeniedState requiredPermission="hr.leave.read" /> : null}
      {state.status === 'error' ? (
        <ErrorState title="Could not load" message={state.message} requestId={state.requestId} />
      ) : null}
      {state.status === 'ready' && state.items.length === 0 ? (
        <EmptyState title="No leave requests" description="Submit a request to get started." />
      ) : null}
      {state.status === 'ready' && state.items.length > 0 ? (
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'start', header: 'Start', cell: (r) => `${r.start_date} (${r.start_period})` },
            { key: 'end', header: 'End', cell: (r) => `${r.end_date} (${r.end_period})` },
            { key: 'days', header: 'Days', cell: (r) => r.units_days },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
            { key: 'reason', header: 'Reason', cell: (r) => r.reason ?? '—' },
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
