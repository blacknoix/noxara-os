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
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Schedule = {
  id: string;
  name: string;
  timezone: string;
  location: string | null;
  is_default: boolean;
};

type Holiday = {
  id: string;
  name: string;
  holiday_date: string;
  is_half_day: boolean;
  location: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; schedules: Schedule[]; holidays: Holiday[] };

export default function SchedulesPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [schedName, setSchedName] = useState('Standard');
  const [timezone, setTimezone] = useState('UTC');
  const [holName, setHolName] = useState('');
  const [holDate, setHolDate] = useState('');
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const [sRes, hRes] = await Promise.all([
      authFetch('/api/v1/people/schedules'),
      authFetch('/api/v1/people/holidays'),
    ]);
    if (sRes.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (sRes.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!sRes.ok || !hRes.ok) {
      setState({ status: 'error', message: 'Could not load schedules.' });
      return;
    }
    const schedules = ((await sRes.json()) as { items: Schedule[] }).items ?? [];
    const holidays = ((await hRes.json()) as { items: Holiday[] }).items ?? [];
    setState({ status: 'ready', schedules, holidays });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function createSchedule(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setBusy(true);
    try {
      const res = await authFetch('/api/v1/people/schedules', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          name: schedName,
          timezone,
          is_default: true,
          weekly_hours: {
            mon: [['09:00', '17:00']],
            tue: [['09:00', '17:00']],
            wed: [['09:00', '17:00']],
            thu: [['09:00', '17:00']],
            fri: [['09:00', '17:00']],
          },
        }),
      });
      if (!res.ok) {
        setFormError('Could not create schedule.');
        return;
      }
      await load();
    } finally {
      setBusy(false);
    }
  }

  async function createHoliday(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setBusy(true);
    try {
      const res = await authFetch('/api/v1/people/holidays', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({ name: holName, holiday_date: holDate }),
      });
      if (!res.ok) {
        setFormError('Could not create holiday.');
        return;
      }
      setHolName('');
      setHolDate('');
      await load();
    } finally {
      setBusy(false);
    }
  }

  if (capsLoading) return <LoadingState label="Loading permissions…" />;
  if (!can('hr.attendance.read')) {
    return <PermissionDeniedState requiredPermission="hr.attendance.read" />;
  }

  return (
    <main style={pageStyle}>
      <header>
        <p style={eyebrowStyle}>
          <Link href="/people">People</Link>
        </p>
        <h1 style={h1Style}>Schedules & holidays</h1>
        <p style={leadStyle}>Work schedules and holiday calendars for leave calculations.</p>
      </header>

      {can('hr.attendance.write') ? (
        <>
          <form style={cardStyle} onSubmit={createSchedule} aria-label="Create schedule">
            <h2 style={h2Style}>Work schedule</h2>
            <label style={labelStyle}>
              Name
              <input
                value={schedName}
                onChange={(e) => setSchedName(e.target.value)}
                style={inputStyle}
                required
              />
            </label>
            <label style={labelStyle}>
              Timezone
              <input
                value={timezone}
                onChange={(e) => setTimezone(e.target.value)}
                style={inputStyle}
                required
              />
            </label>
            <Button type="submit" disabled={busy}>
              Save schedule
            </Button>
          </form>
          <form style={cardStyle} onSubmit={createHoliday} aria-label="Create holiday">
            <h2 style={h2Style}>Holiday</h2>
            <label style={labelStyle}>
              Name
              <input
                value={holName}
                onChange={(e) => setHolName(e.target.value)}
                style={inputStyle}
                required
              />
            </label>
            <label style={labelStyle}>
              Date
              <input
                type="date"
                value={holDate}
                onChange={(e) => setHolDate(e.target.value)}
                style={inputStyle}
                required
              />
            </label>
            <Button type="submit" disabled={busy}>
              Add holiday
            </Button>
          </form>
        </>
      ) : null}
      {formError ? <p style={errorText}>{formError}</p> : null}

      {state.status === 'loading' ? <LoadingState label="Loading…" /> : null}
      {state.status === 'signed_out' ? (
        <ErrorState title="Sign in required" message="Sign in to view schedules." />
      ) : null}
      {state.status === 'denied' ? (
        <PermissionDeniedState requiredPermission="hr.attendance.read" />
      ) : null}
      {state.status === 'error' ? (
        <ErrorState title="Could not load" message={state.message} />
      ) : null}
      {state.status === 'ready' ? (
        <>
          <h2 style={h2Style}>Schedules</h2>
          {state.schedules.length === 0 ? (
            <EmptyState title="No schedules" description="Create a default work schedule." />
          ) : (
            <Table
              getRowKey={(r) => r.id}
              columns={[
                { key: 'name', header: 'Name', cell: (r) => r.name },
                { key: 'tz', header: 'Timezone', cell: (r) => r.timezone },
                {
                  key: 'default',
                  header: 'Default',
                  cell: (r) => (r.is_default ? 'Yes' : 'No'),
                },
              ]}
              rows={state.schedules}
            />
          )}
          <h2 style={h2Style}>Holidays</h2>
          {state.holidays.length === 0 ? (
            <EmptyState title="No holidays" description="Add public holidays for the org." />
          ) : (
            <Table
              getRowKey={(r) => r.id}
              columns={[
                { key: 'date', header: 'Date', cell: (r) => r.holiday_date },
                { key: 'name', header: 'Name', cell: (r) => r.name },
                {
                  key: 'half',
                  header: 'Half day',
                  cell: (r) => (r.is_half_day ? 'Yes' : 'No'),
                },
              ]}
              rows={state.holidays}
            />
          )}
        </>
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
