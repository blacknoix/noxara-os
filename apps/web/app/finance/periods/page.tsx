'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
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

type FiscalPeriod = {
  id: string;
  code: string;
  name: string;
  start_date: string;
  end_date: string;
  status: string;
  checklist: Record<string, boolean> | Record<string, unknown>;
  closed_at?: string | null;
  reopened_at?: string | null;
  reopen_reason?: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: FiscalPeriod[] };

function formatChecklist(checklist: FiscalPeriod['checklist']): string {
  if (!checklist || typeof checklist !== 'object') return '—';
  const entries = Object.entries(checklist);
  if (entries.length === 0) return '—';
  return entries
    .map(([key, value]) => `${key}: ${value === true ? 'yes' : value === false ? 'no' : String(value)}`)
    .join(' · ');
}

export default function PeriodsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [busyId, setBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/periods');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load periods.' });
      return;
    }
    const body = (await res.json()) as { items: FiscalPeriod[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function closePeriod(id: string) {
    setActionError(null);
    setBusyId(id);
    try {
      const res = await authFetch(`/api/v1/finance/periods/${encodeURIComponent(id)}/close`, {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setActionError(body.detail ?? 'Could not close period.');
        return;
      }
      await load();
    } finally {
      setBusyId(null);
    }
  }

  async function reopenPeriod(id: string) {
    setActionError(null);
    const reason = window.prompt('Reason for reopening this period?');
    if (reason === null) return;
    if (!reason.trim()) {
      setActionError('A reopen reason is required.');
      return;
    }
    setBusyId(id);
    try {
      const res = await authFetch(`/api/v1/finance/periods/${encodeURIComponent(id)}/reopen`, {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({ reason: reason.trim() }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setActionError(body.detail ?? 'Could not reopen period.');
        return;
      }
      await load();
    } finally {
      setBusyId(null);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading periods…" />;
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
    return <PermissionDeniedState requiredPermission="finance.period.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Periods unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Fiscal periods</h1>
          <p style={muted}>Open, close, and reopen accounting periods.</p>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}>
          <Button type="button" variant="ghost">
            Back
          </Button>
        </Link>
      </header>

      {actionError ? <ErrorState message={actionError} /> : null}

      {state.items.length === 0 ? (
        <EmptyState title="No periods" description="Periods are created when journals post." />
      ) : (
        <Table
          getRowKey={(r: FiscalPeriod) => r.id}
          columns={[
            { key: 'code', header: 'Code', cell: (r: FiscalPeriod) => r.code },
            { key: 'name', header: 'Name', cell: (r: FiscalPeriod) => r.name },
            {
              key: 'range',
              header: 'Range',
              cell: (r: FiscalPeriod) => `${r.start_date} → ${r.end_date}`,
            },
            {
              key: 'status',
              header: 'Status',
              cell: (r: FiscalPeriod) => <Badge>{r.status}</Badge>,
            },
            {
              key: 'checklist',
              header: 'Checklist',
              cell: (r: FiscalPeriod) => (
                <span style={{ fontSize: '0.8rem', color: 'var(--cos-color-fg-muted)' }}>
                  {formatChecklist(r.checklist)}
                </span>
              ),
            },
            {
              key: 'actions',
              header: 'Actions',
              cell: (r: FiscalPeriod) => (
                <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  {r.status === 'open' ? (
                    <Button
                      type="button"
                      variant="secondary"
                      disabled={busyId === r.id}
                      onClick={() => void closePeriod(r.id)}
                    >
                      Close
                    </Button>
                  ) : null}
                  {r.status === 'closed' ? (
                    <Button
                      type="button"
                      variant="ghost"
                      disabled={busyId === r.id}
                      onClick={() => void reopenPeriod(r.id)}
                    >
                      Reopen
                    </Button>
                  ) : null}
                </div>
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
