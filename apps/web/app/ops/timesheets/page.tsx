'use client';

import { useCallback, useEffect, useMemo, useState, type CSSProperties, type FormEvent } from 'react';
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

type TimeEntry = {
  id: string;
  entry_date: string;
  minutes: number;
  billable: boolean;
  project_id: string;
  status: string;
};

type Timesheet = {
  id: string;
  membership_user_id: string;
  week_start: string;
  status: string;
  entries: TimeEntry[];
  version: number;
};

function mondayOf(d = new Date()): string {
  const copy = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = copy.getUTCDay();
  const diff = day === 0 ? -6 : 1 - day;
  copy.setUTCDate(copy.getUTCDate() + diff);
  return copy.toISOString().slice(0, 10);
}

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'approved':
      return 'success';
    case 'submitted':
      return 'info';
    case 'rejected':
      return 'danger';
    default:
      return 'neutral';
  }
}

export default function TimesheetsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [sheets, setSheets] = useState<Timesheet[]>([]);
  const [selected, setSelected] = useState<Timesheet | null>(null);
  const [projects, setProjects] = useState<Array<{ id: string; name: string }>>([]);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [entryDate, setEntryDate] = useState('');
  const [minutes, setMinutes] = useState('60');
  const [projectId, setProjectId] = useState('');
  const [billable, setBillable] = useState(true);

  const weekStart = useMemo(() => mondayOf(), []);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    const [tsRes, prRes] = await Promise.all([
      authFetch('/api/v1/operations/timesheets?limit=50'),
      authFetch('/api/v1/operations/projects?limit=50'),
    ]);
    if (tsRes.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!tsRes.ok) {
      setError('Could not load timesheets');
      setLoading(false);
      return;
    }
    const body = await tsRes.json();
    const items: Timesheet[] = body.items ?? [];
    setSheets(items);
    if (items[0]) setSelected(items[0]);
    if (prRes.ok) {
      const pBody = await prRes.json();
      setProjects(pBody.items ?? []);
      if ((pBody.items ?? [])[0]) setProjectId(pBody.items[0].id);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (selected) setEntryDate(selected.week_start);
  }, [selected]);

  async function createWeek() {
    setBusy(true);
    const res = await authFetch('/api/v1/operations/timesheets', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ week_start: weekStart }),
    });
    setBusy(false);
    if (res.ok) {
      const sheet = (await res.json()) as Timesheet;
      setSelected(sheet);
      await load();
    }
  }

  async function addEntry(e: FormEvent) {
    e.preventDefault();
    if (!selected || selected.status !== 'draft') return;
    setBusy(true);
    const res = await authFetch(
      `/api/v1/operations/timesheets/${encodeURIComponent(selected.id)}/entries`,
      {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          project_id: projectId,
          entry_date: entryDate,
          minutes: Number(minutes),
          billable,
        }),
      },
    );
    setBusy(false);
    if (res.ok) {
      const sheet = (await res.json()) as Timesheet;
      setSelected(sheet);
      await load();
    }
  }

  async function submitSheet() {
    if (!selected) return;
    setBusy(true);
    const res = await authFetch(
      `/api/v1/operations/timesheets/${encodeURIComponent(selected.id)}/submit`,
      { method: 'POST' },
    );
    setBusy(false);
    if (res.ok) {
      setSelected((await res.json()) as Timesheet);
      await load();
    }
  }

  async function approveSheet() {
    if (!selected || !can('operations.timesheet.approve')) return;
    setBusy(true);
    const res = await authFetch(
      `/api/v1/operations/timesheets/${encodeURIComponent(selected.id)}/approve`,
      { method: 'POST' },
    );
    setBusy(false);
    if (res.ok) {
      setSelected((await res.json()) as Timesheet);
      await load();
    }
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view timesheets." />;
  }
  if (capsLoading || loading) return <LoadingState label="Loading timesheets…" />;
  if (denied || !can('operations.timesheet.read')) {
    return <PermissionDeniedState requiredPermission="operations.timesheet.read" />;
  }
  if (error) return <ErrorState title="Timesheets unavailable" message={error} />;

  const totalMinutes = selected?.entries.reduce((n, e) => n + e.minutes, 0) ?? 0;

  return (
    <div style={page}>
      <header style={header}>
        <div>
          <p style={eyebrow}>Operations</p>
          <h1 style={title}>Timesheets</h1>
          <p style={muted}>Week grid on time entries — submit for approval; managers approve.</p>
        </div>
        {can('operations.timesheet.write') ? (
          <Button onClick={() => void createWeek()} disabled={busy}>
            New week ({weekStart})
          </Button>
        ) : null}
      </header>

      <div style={layout}>
        <aside style={aside}>
          {sheets.length === 0 ? (
            <EmptyState title="No timesheets" description="Create a week sheet to start logging." />
          ) : (
            <ul style={list}>
              {sheets.map((s) => (
                <li key={s.id}>
                  <button
                    type="button"
                    style={s.id === selected?.id ? listItemActive : listItem}
                    onClick={() => setSelected(s)}
                  >
                    <span>{s.week_start}</span>
                    <StatusCell status={s.status} tone={statusTone(s.status)} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </aside>

        <section style={section}>
          {!selected ? (
            <p style={muted}>Select a timesheet.</p>
          ) : (
            <>
              <div style={toolbar}>
                <div>
                  <strong>Week of {selected.week_start}</strong>
                  <p style={muted}>
                    {totalMinutes} minutes · {(totalMinutes / 60).toFixed(1)} hours
                  </p>
                </div>
                <div style={{ display: 'flex', gap: '0.5rem' }}>
                  {selected.status === 'draft' && can('operations.timesheet.submit') ? (
                    <Button onClick={() => void submitSheet()} disabled={busy}>
                      Submit
                    </Button>
                  ) : null}
                  {selected.status === 'submitted' && can('operations.timesheet.approve') ? (
                    <Button onClick={() => void approveSheet()} disabled={busy}>
                      Approve
                    </Button>
                  ) : null}
                </div>
              </div>

              {selected.status === 'draft' && can('operations.timesheet.write') ? (
                <form onSubmit={addEntry} style={form}>
                  <select
                    value={projectId}
                    onChange={(e) => setProjectId(e.target.value)}
                    style={input}
                  >
                    {projects.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                  <input
                    type="date"
                    value={entryDate}
                    onChange={(e) => setEntryDate(e.target.value)}
                    style={input}
                  />
                  <input
                    type="number"
                    min={1}
                    value={minutes}
                    onChange={(e) => setMinutes(e.target.value)}
                    style={{ ...input, maxWidth: '6rem' }}
                    aria-label="Minutes"
                  />
                  <label style={{ display: 'flex', alignItems: 'center', gap: '0.35rem' }}>
                    <input
                      type="checkbox"
                      checked={billable}
                      onChange={(e) => setBillable(e.target.checked)}
                    />
                    Billable
                  </label>
                  <Button type="submit" disabled={busy || !projectId}>
                    Add entry
                  </Button>
                </form>
              ) : null}

              <Table
                getRowKey={(e: TimeEntry) => e.id}
                columns={[
                  { key: 'date', header: 'Date', cell: (e: TimeEntry) => e.entry_date },
                  {
                    key: 'mins',
                    header: 'Minutes',
                    cell: (e: TimeEntry) => String(e.minutes),
                  },
                  {
                    key: 'billable',
                    header: 'Billable',
                    cell: (e: TimeEntry) => (e.billable ? 'Yes' : 'No'),
                  },
                  { key: 'project', header: 'Project', cell: (e: TimeEntry) => e.project_id },
                ]}
                rows={selected.entries}
              />
            </>
          )}
        </section>
      </div>
    </div>
  );
}

const page: CSSProperties = { padding: '1.5rem', display: 'grid', gap: '1.25rem' };
const header: CSSProperties = {
  display: 'flex',
  justifyContent: 'space-between',
  alignItems: 'flex-end',
  gap: '1rem',
};
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.08em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };
const layout: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(12rem, 16rem) 1fr',
  gap: '1.25rem',
};
const aside: CSSProperties = { display: 'grid', gap: '0.5rem', alignContent: 'start' };
const list: CSSProperties = { listStyle: 'none', margin: 0, padding: 0, display: 'grid', gap: '0.35rem' };
const listItem: CSSProperties = {
  width: '100%',
  display: 'flex',
  justifyContent: 'space-between',
  gap: '0.5rem',
  padding: '0.6rem 0.75rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
};
const listItemActive: CSSProperties = {
  ...listItem,
  borderColor: 'var(--cos-color-accent)',
  background: 'var(--cos-color-bg-muted)',
};
const section: CSSProperties = { display: 'grid', gap: '0.75rem' };
const toolbar: CSSProperties = {
  display: 'flex',
  justifyContent: 'space-between',
  alignItems: 'center',
  gap: '1rem',
};
const form: CSSProperties = { display: 'flex', flexWrap: 'wrap', gap: '0.5rem', alignItems: 'center' };
const input: CSSProperties = {
  padding: '0.45rem 0.65rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg)',
  color: 'var(--cos-color-fg)',
};
