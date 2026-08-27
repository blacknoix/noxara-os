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

type Attendance = {
  id: string;
  employee_id: string;
  entry_kind: string;
  recorded_at: string;
  local_date: string;
  source: string;
  note: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; items: Attendance[] };

export default function AttendancePage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [note, setNote] = useState('');
  const [useGeo, setUseGeo] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/people/me/attendance?limit=50');
    const requestId = res.headers.get('x-request-id') ?? undefined;
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load attendance.', requestId });
      return;
    }
    const body = (await res.json()) as { items: Attendance[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function clock(entryKind: 'check_in' | 'check_out') {
    setFormError(null);
    setBusy(true);
    try {
      let latitude: number | undefined;
      let longitude: number | undefined;
      let accuracy_meters: number | undefined;
      let source = 'manual';
      if (useGeo && typeof navigator !== 'undefined' && navigator.geolocation) {
        const pos = await new Promise<GeolocationPosition>((resolve, reject) => {
          navigator.geolocation.getCurrentPosition(resolve, reject, {
            enableHighAccuracy: false,
            timeout: 5000,
            maximumAge: 60_000,
          });
        }).catch(() => null);
        if (pos) {
          latitude = pos.coords.latitude;
          longitude = pos.coords.longitude;
          accuracy_meters = pos.coords.accuracy;
          source = 'geo';
        }
      }
      const res = await authFetch('/api/v1/people/attendance', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          entry_kind: entryKind,
          source,
          latitude,
          longitude,
          accuracy_meters,
          note: note.trim() || null,
        }),
      });
      if (!res.ok) {
        let message = 'Could not record attendance.';
        try {
          const body = await res.json();
          if (typeof body.detail === 'string') message = body.detail;
        } catch {
          /* ignore */
        }
        setFormError(message);
        return;
      }
      setNote('');
      await load();
    } finally {
      setBusy(false);
    }
  }

  if (capsLoading) return <LoadingState label="Loading permissions…" />;
  if (!can('hr.attendance.read') && state.status !== 'denied') {
    return <PermissionDeniedState requiredPermission="hr.attendance.read" />;
  }

  return (
    <main style={pageStyle}>
      <header style={headerStyle}>
        <div>
          <p style={eyebrowStyle}>
            <Link href="/people">People</Link>
          </p>
          <h1 style={h1Style}>Attendance</h1>
          <p style={leadStyle}>Clock in or out. Facts are append-only; corrections create reversing entries.</p>
        </div>
      </header>

      {can('hr.attendance.write') ? (
        <section style={cardStyle} aria-label="Clock">
          <h2 style={h2Style}>Clock</h2>
          <label style={labelStyle}>
            Note (optional)
            <input
              value={note}
              onChange={(e) => setNote(e.target.value)}
              style={inputStyle}
            />
          </label>
          <label style={{ ...labelStyle, flexDirection: 'row', alignItems: 'center', gap: 8 }}>
            <input
              type="checkbox"
              checked={useGeo}
              onChange={(e) => setUseGeo(e.target.checked)}
            />
            Include approximate location (lat/lng + accuracy only)
          </label>
          {formError ? <p style={errorText}>{formError}</p> : null}
          <div style={{ display: 'flex', gap: 12 }}>
            <Button type="button" disabled={busy} onClick={() => void clock('check_in')}>
              Check in
            </Button>
            <Button type="button" disabled={busy} onClick={() => void clock('check_out')}>
              Check out
            </Button>
          </div>
        </section>
      ) : null}

      {state.status === 'loading' ? <LoadingState label="Loading attendance…" /> : null}
      {state.status === 'signed_out' ? (
        <ErrorState title="Sign in required" message="Sign in to view attendance." />
      ) : null}
      {state.status === 'denied' ? (
        <PermissionDeniedState requiredPermission="hr.attendance.read" />
      ) : null}
      {state.status === 'error' ? (
        <ErrorState title="Could not load" message={state.message} requestId={state.requestId} />
      ) : null}
      {state.status === 'ready' && state.items.length === 0 ? (
        <EmptyState title="No attendance yet" description="Clock in to create your first record." />
      ) : null}
      {state.status === 'ready' && state.items.length > 0 ? (
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'local_date', header: 'Date', cell: (r) => r.local_date },
            {
              key: 'entry_kind',
              header: 'Kind',
              cell: (r) => <StatusCell status={r.entry_kind} />,
            },
            {
              key: 'recorded_at',
              header: 'Recorded',
              cell: (r) => new Date(r.recorded_at).toLocaleString(),
            },
            { key: 'source', header: 'Source', cell: (r) => r.source },
            { key: 'note', header: 'Note', cell: (r) => r.note ?? '—' },
          ]}
          rows={state.items}
        />
      ) : null}
    </main>
  );
}

const pageStyle: CSSProperties = { display: 'grid', gap: 24, padding: '8px 0 48px' };
const headerStyle: CSSProperties = { display: 'flex', justifyContent: 'space-between', gap: 16 };
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
